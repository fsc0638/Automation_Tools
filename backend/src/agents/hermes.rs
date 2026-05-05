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

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
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

    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let mut full = vec![ChatMessage { role: "system".into(), content: Self::system_prompt().into() }];
        full.extend(messages);

        let res = self.client
            .post(format!("{}/chat/completions", self.api_url))
            .bearer_auth(&self.api_key)
            .json(&ChatRequest { model: self.model.clone(), messages: full, stream: false, temperature: 0.6 })
            .send().await?.error_for_status()?.json::<ChatResponse>().await?;

        Ok(res.choices.into_iter().next().map(|c| c.message.content).unwrap_or_default())
    }

    pub fn chat_stream(&self, messages: Vec<ChatMessage>) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        let client = self.client.clone();
        let url = format!("{}/chat/completions", self.api_url);
        let api_key = self.api_key.clone();
        let model = self.model.clone();
        let mut full = vec![ChatMessage { role: "system".into(), content: Self::system_prompt().into() }];
        full.extend(messages);

        Box::pin(async_stream::stream! {
            let req = ChatRequest { model, messages: full, stream: true, temperature: 0.6 };
            let response = match client.post(&url).bearer_auth(&api_key).json(&req).send().await {
                Ok(r) => r,
                Err(e) => { yield format!("[Hermes error: {}]", e); return; }
            };
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = match chunk { Ok(c) => c, Err(_) => break };
                let text = String::from_utf8_lossy(&chunk).to_string();
                for line in text.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" { return; }
                        if let Ok(parsed) = serde_json::from_str::<StreamChunk>(data) {
                            for choice in parsed.choices {
                                if let Some(content) = choice.delta.content {
                                    yield content;
                                }
                            }
                        }
                    }
                }
            }
        })
    }
}
