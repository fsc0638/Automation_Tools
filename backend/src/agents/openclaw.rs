use anyhow::{Context, Result};
use futures_util::{Stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

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
    choices: Option<Vec<StreamChoice>>,
}

#[derive(Clone)]
pub struct OpenClawClient {
    client: Client,
    chat_completions_url: String,
    gateway_token: String,
    model: String,
}

impl OpenClawClient {
    pub fn new(config: &Config) -> Self {
        Self {
            client: Client::new(),
            chat_completions_url: Self::chat_completions_url(&config.openclaw_api_url),
            gateway_token: config.openclaw_api_key.clone(),
            model: config.openclaw_model.clone(),
        }
    }

    pub fn system_prompt() -> &'static str {
        "You are OpenClaw, a fixed and independent GPT-5.5 software-engineering agent. \
        Identity is important: you are not Hermes, and you must keep your own technical judgment. \
        Default language: Traditional Chinese. Only use English when the user explicitly asks for English, \
        or when preserving code/API/error text. \
        Your strengths are deep codebase analysis, complex debugging, race conditions, intermittent bugs, \
        implementation planning, architecture-impact analysis, and high-leverage technical decisions. \
        You may disagree with Hermes clearly when its advice is incomplete, risky, too abstract, or merely compromising. \
        Be concise: prefer short sections, bullets, and concrete actions over long explanations. \
        If code changes are requested in Debate Mode, treat intermediate rounds as analysis only; \
        the Final answer is authoritative for implementation details, file paths, diffs, commands, and tests. \
        Project isolation is mandatory: never let another project's files, answers, architecture, or decisions affect the current project. \
        Shared learning is limited to general engineering skill and reasoning patterns. \
        GPT-5.5 can hallucinate more confidently, so never claim something is fixed without verification. \
        Format code blocks with proper markdown and language tags."
    }

    fn chat_completions_url(api_url: &str) -> String {
        let base = api_url.trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/chat/completions")
        }
    }

    fn build_messages(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
        let mut full = vec![ChatMessage {
            role: "system".into(),
            content: Self::system_prompt().into(),
        }];

        full.extend(messages.into_iter().filter_map(|msg| {
            let role = match msg.role.as_str() {
                "system" | "developer" | "user" | "assistant" => msg.role,
                "openclaw" | "hermes" => "assistant".into(),
                _ => return None,
            };

            Some(ChatMessage {
                role,
                content: msg.content,
            })
        }));

        full
    }

    fn request(&self, messages: Vec<ChatMessage>, stream: bool) -> ChatRequest {
        ChatRequest {
            // Gateway treats `model` as an agent target. The actual provider model
            // is sent with `x-openclaw-model` below.
            model: "openclaw/default".into(),
            messages: Self::build_messages(messages),
            stream,
        }
    }

    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let response = self
            .client
            .post(&self.chat_completions_url)
            .bearer_auth(&self.gateway_token)
            .header("x-openclaw-model", &self.model)
            .json(&self.request(messages, false))
            .send()
            .await
            .context("failed to send request to OpenClaw Gateway")?
            .error_for_status()
            .context("OpenClaw Gateway returned an error status")?
            .json::<ChatResponse>()
            .await
            .context("failed to parse OpenClaw Gateway response")?;

        Ok(response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .unwrap_or_default())
    }

    pub fn chat_stream(&self, messages: Vec<ChatMessage>) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        let client = self.client.clone();
        let url = self.chat_completions_url.clone();
        let token = self.gateway_token.clone();
        let provider_model = self.model.clone();
        let request = self.request(messages, true);

        Box::pin(async_stream::stream! {
            let response = match client
                .post(&url)
                .bearer_auth(&token)
                .header("x-openclaw-model", &provider_model)
                .json(&request)
                .send()
                .await
            {
                Ok(response) => match response.error_for_status() {
                    Ok(response) => response,
                    Err(error) => {
                        yield format!("[OpenClaw error: Gateway returned an error status: {error}]");
                        return;
                    }
                },
                Err(error) => {
                    yield format!("[OpenClaw error: failed to send request to Gateway: {error}]");
                    return;
                }
            };

            let mut bytes = response.bytes_stream();
            let mut buffer = String::new();
            let mut current_event: Option<String> = None;

            while let Some(chunk) = bytes.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        yield format!("[OpenClaw stream error: {error}]");
                        return;
                    }
                };

                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if line.is_empty() {
                        current_event = None;
                        continue;
                    }

                    if let Some(event_name) = line.strip_prefix("event:").map(str::trim) {
                        current_event = Some(event_name.to_string());
                        continue;
                    }

                    let Some(data) = line.strip_prefix("data:").map(str::trim) else {
                        continue;
                    };

                    if data == "[DONE]" {
                        return;
                    }

                    if matches!(current_event.as_deref(), Some("hermes.tool.progress")) {
                        current_event = None;
                        continue;
                    }

                    match serde_json::from_str::<StreamChunk>(data) {
                        Ok(parsed) => {
                            if let Some(choices) = parsed.choices {
                                for choice in choices {
                                    if let Some(content) = choice.delta.content {
                                        yield content;
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            current_event = None;
                            continue;
                        }
                    }

                    current_event = None;
                }
            }
        })
    }
}
