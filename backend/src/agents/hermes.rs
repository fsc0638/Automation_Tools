use anyhow::{Context, Result};
use futures_util::{Stream, StreamExt};
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use std::pin::Pin;

use crate::{agents::openclaw::ChatMessage, config::Config};

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

/// Hard ceiling on a single agent reply. See OpenClawClient for rationale.
const REPLY_TOKEN_CEILING: u32 = 1200;

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
pub struct HermesClient {
    client: Client,
    chat_completions_url: String,
    api_key: String,
    model: String,
}

impl HermesClient {
    pub fn new(config: &Config) -> Self {
        Self {
            client: Client::new(),
            chat_completions_url: Self::chat_completions_url(&config.hermes_api_url),
            api_key: config.hermes_api_key.clone(),
            model: config.hermes_model.clone(),
        }
    }

    pub fn system_prompt() -> &'static str {
        "You are Hermes, a fixed and independent GPT-5.4 pragmatic engineering agent. \
        Identity is important: you are not OpenClaw, and you must keep your own technical judgment. \
        Default language: Traditional Chinese. Only use English when the user explicitly asks for English, \
        or when preserving code/API/error text. \
        Your strengths are day-to-day development, cost-efficient implementation, quick iteration, \
        straightforward debugging, code review, and turning plans into practical steps. \
        You may disagree with OpenClaw clearly when its plan is over-engineered, expensive, speculative, \
        not grounded in code, or merely forcing compromise. \
        Reply style: conversational Traditional Chinese, plain-language, focused on key points. \
        If code changes are requested in Debate Mode, intermediate rounds are analysis only; \
        the Final answer is authoritative for implementation details. \
        Project isolation is mandatory: never let another project's files, answers, architecture, or decisions affect the current project. \
        Shared learning is limited to general engineering skill and reasoning patterns. \
        Prefer stable, maintainable, incremental solutions and call out when a task should be escalated to OpenClaw/GPT-5.5. \
        Format code blocks with proper markdown. \
        \
        RESPONSE LENGTH (hard rules — these override all other style guidance): \
        Target roughly 80% of your usual length. Aim for ≤3 bullets OR ≤180 Traditional Chinese characters per turn, whichever is shorter. \
        First sentence carries the bottom-line conclusion. No greetings, no recap of these rules, no '首先/其次/最後/總結' filler. \
        Compress repeated explanations into a single line; drop tangential context. \
        For the Debate Final you may use up to ≤6 bullets when delivering an implementation plan; otherwise keep to the default cap. \
        Expand beyond these caps only when the user explicitly asks '詳細', '展開', or '完整'."
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
            // Hermes is an independent local agent/gateway. This is its own model
            // or agent target, not an OpenClaw Gateway target.
            model: self.model.clone(),
            messages: Self::build_messages(messages),
            stream,
            max_tokens: Some(REPLY_TOKEN_CEILING),
        }
    }

    fn with_auth(&self, request: RequestBuilder) -> RequestBuilder {
        if self.api_key.trim().is_empty() {
            request
        } else {
            request.bearer_auth(&self.api_key)
        }
    }

    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let response = self
            .with_auth(self.client.post(&self.chat_completions_url))
            .json(&self.request(messages, false))
            .send()
            .await
            .context("failed to send request to Hermes Agent")?
            .error_for_status()
            .context("Hermes Agent returned an error status")?
            .json::<ChatResponse>()
            .await
            .context("failed to parse Hermes Agent response")?;

        Ok(response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .unwrap_or_default())
    }

    pub fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
    ) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        let client = self.client.clone();
        let url = self.chat_completions_url.clone();
        let api_key = self.api_key.clone();
        let request = self.request(messages, true);

        Box::pin(async_stream::stream! {
            let mut builder = client.post(&url).json(&request);
            if !api_key.trim().is_empty() {
                builder = builder.bearer_auth(&api_key);
            }

            let response = match builder.send().await {
                Ok(response) => match response.error_for_status() {
                    Ok(response) => response,
                    Err(error) => {
                        yield format!("[Hermes error: Agent returned an error status: {error}]");
                        return;
                    }
                },
                Err(error) => {
                    yield format!("[Hermes error: failed to send request to Agent: {error}]");
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
                        yield format!("[Hermes stream error: {error}]");
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
