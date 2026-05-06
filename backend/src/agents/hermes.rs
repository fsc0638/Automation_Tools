use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use futures_util::StreamExt;
use std::pin::Pin;
use futures_util::Stream;

use crate::{agents::openclaw::ChatMessage, config::Config};

#[derive(Clone)]
pub struct HermesClient {
    client: Client,
    api_url: String,
    api_key: String,
    model: String,
}

// Anthropic /v1/messages request
#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String, // "user" | "assistant"
    content: String,
}

// Non-streaming response
#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

// Streaming SSE events
#[derive(Debug, Deserialize)]
struct StreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    delta: Option<StreamDelta>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    text: Option<String>,
}

impl HermesClient {
    pub fn new(config: &Config) -> Self {
        Self {
            client: Client::new(),
            api_url: config.hermes_api_url.clone(),
            api_key: config.hermes_api_key.clone(),
            model: config.hermes_model.clone(),
        }
    }

    pub fn system_prompt() -> &'static str {
        "You are Hermes, a senior software architect AI assistant. \
        Your role is to provide high-level architectural insights, identify code patterns, \
        design trade-offs, and strategic guidance. You collaborate with OpenClaw, \
        a code-focused agent. Be analytical, thorough, and constructive. \
        When reviewing OpenClaw's suggestions, provide critical feedback and alternative approaches. \
        Format code blocks with proper markdown."
    }

    fn to_anthropic_messages(messages: Vec<ChatMessage>) -> Vec<AnthropicMessage> {
        messages
            .into_iter()
            .filter(|m| m.role != "system")
            .map(|m| AnthropicMessage {
                role: if m.role == "user" { "user".into() } else { "assistant".into() },
                content: m.content,
            })
            .collect()
    }

    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let req = AnthropicRequest {
            model: self.model.clone(),
            max_tokens: 8192,
            system: Self::system_prompt().into(),
            messages: Self::to_anthropic_messages(messages),
            stream: false,
        };

        let res: AnthropicResponse = self.client
            .post(format!("{}/v1/messages", self.api_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&req)
            .send().await?
            .error_for_status()?
            .json().await?;

        Ok(res.content.into_iter()
            .filter(|b| b.block_type == "text")
            .filter_map(|b| b.text)
            .collect::<Vec<_>>()
            .join(""))
    }

    pub fn chat_stream(&self, messages: Vec<ChatMessage>) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        let client = self.client.clone();
        let url = format!("{}/v1/messages", self.api_url);
        let api_key = self.api_key.clone();
        let model = self.model.clone();
        let anthropic_messages = Self::to_anthropic_messages(messages);

        Box::pin(async_stream::stream! {
            let req = AnthropicRequest {
                model,
                max_tokens: 8192,
                system: Self::system_prompt().into(),
                messages: anthropic_messages,
                stream: true,
            };

            let response = match client
                .post(&url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&req)
                .send().await
            {
                Ok(r) => r,
                Err(e) => { yield format!("[Hermes error: {}]", e); return; }
            };

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                yield format!("[Hermes error: {} — {}]", status, body);
                return;
            }

            let mut stream = response.bytes_stream();
            let mut buf = String::new();

            while let Some(chunk) = stream.next().await {
                let chunk = match chunk { Ok(c) => c, Err(_) => break };
                buf.push_str(&String::from_utf8_lossy(&chunk));

                // Process complete lines from buffer
                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].trim().to_string();
                    buf = buf[pos + 1..].to_string();

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" { return; }
                        if let Ok(event) = serde_json::from_str::<StreamEvent>(data) {
                            if event.event_type == "content_block_delta" {
                                if let Some(delta) = event.delta {
                                    if delta.delta_type.as_deref() == Some("text_delta") {
                                        if let Some(text) = delta.text {
                                            yield text;
                                        }
                                    }
                                }
                            }
                            if event.event_type == "message_stop" { return; }
                        }
                    }
                }
            }
        })
    }
}
