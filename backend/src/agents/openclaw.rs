use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use futures_util::StreamExt;
use std::pin::Pin;
use futures_util::Stream;

use crate::config::Config;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
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

#[derive(Clone)]
pub struct OpenClawClient {
    client: Client,
    api_url: String,
    api_key: String,
    model: String,
}

impl OpenClawClient {
    pub fn new(config: &Config) -> Self {
        Self {
            client: Client::new(),
            api_url: config.openclaw_api_url.clone(),
            api_key: config.openclaw_api_key.clone(),
            model: config.openclaw_model.clone(),
        }
    }

    pub fn system_prompt() -> &'static str {
        "You are OpenClaw, an expert software engineer AI assistant. \
        Your role is to write clean, efficient, production-ready code, \
        identify bugs, and provide concrete implementation solutions. \
        You collaborate with Hermes, an architectural AI. \
        Format code blocks with proper markdown and language tags."
    }

    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let mut full = vec![ChatMessage { role: "system".into(), content: Self::system_prompt().into() }];
        full.extend(messages);

        let res = self.client
            .post(format!("{}/chat/completions", self.api_url))
            .bearer_auth(&self.api_key)
            .json(&ChatRequest { model: self.model.clone(), messages: full, stream: false, temperature: 0.7 })
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
            let req = ChatRequest { model, messages: full, stream: true, temperature: 0.7 };
            let response = match client.post(&url).bearer_auth(&api_key).json(&req).send().await {
                Ok(r) => r,
                Err(e) => { yield format!("[OpenClaw error: {}]", e); return; }
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
