use anyhow::{Context, Result};
use futures_util::{Stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

use crate::{
    agents::telemetry::{
        parse_openai_stream_chunk, AgentResponseMetadata, AgentStreamEvent, OpenAiStreamChunk,
        OpenAiUsage,
    },
    config::Config,
};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

#[derive(Debug, Serialize)]
struct StreamOptions {
    include_usage: bool,
}

/// Hard ceiling on a single agent reply. Combined with the RESPONSE LENGTH
/// rules in the system prompt, this targets roughly 80% of the previous
/// average reply length while leaving headroom for the Debate Final
/// synthesis to still complete.
const REPLY_TOKEN_CEILING: u32 = 1200;

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<OpenAiUsage>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChatMessage,
}

pub(crate) fn parse_stream_chunk(data: &str) -> serde_json::Result<OpenAiStreamChunk> {
    parse_openai_stream_chunk(data)
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
        Reply style: conversational Traditional Chinese, plain-language, focused on key points. \
        If code changes are requested in Debate Mode, treat intermediate rounds as analysis only; \
        the Final answer is authoritative for implementation details, file paths, diffs, commands, and tests. \
        Project isolation is mandatory: never let another project's files, answers, architecture, or decisions affect the current project. \
        Shared learning is limited to general engineering skill and reasoning patterns. \
        GPT-5.5 can hallucinate more confidently, so never claim something is fixed without verification. \
        Format code blocks with proper markdown and language tags. \
        \
        RESPONSE LENGTH (hard rules — these override all other style guidance): \
        Target roughly 80% of your usual length. Aim for ≤4 bullets OR ≤200 Traditional Chinese characters per turn, whichever is shorter. \
        First sentence carries the bottom-line conclusion. No greetings, no recap of these rules, no '首先/其次/最後/總而言之' filler. \
        Compress repeated explanations into a single line; drop tangential context. \
        For the Debate Final you may use up to ≤7 bullets when delivering an implementation plan; otherwise keep to the default cap. \
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
            // Gateway treats `model` as an agent target. The actual provider model
            // is sent with `x-openclaw-model` below.
            model: "openclaw/default".into(),
            messages: Self::build_messages(messages),
            stream,
            max_tokens: Some(REPLY_TOKEN_CEILING),
            stream_options: stream.then_some(StreamOptions { include_usage: true }),
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

    pub fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
    ) -> Pin<Box<dyn Stream<Item = AgentStreamEvent> + Send>> {
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
                        yield AgentStreamEvent::Content(format!("[OpenClaw error: Gateway returned an error status: {error}]"));
                        return;
                    }
                },
                Err(error) => {
                    yield AgentStreamEvent::Content(format!("[OpenClaw error: failed to send request to Gateway: {error}]"));
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
                        yield AgentStreamEvent::Content(format!("[OpenClaw stream error: {error}]"));
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

                    match parse_stream_chunk(data) {
                        Ok(parsed) => {
                            if let Some(choices) = parsed.choices {
                                for choice in choices {
                                    if let Some(content) = choice.delta.content {
                                        yield AgentStreamEvent::Content(content);
                                    }
                                }
                            }
                            if let Some(usage) = parsed.usage {
                                yield AgentStreamEvent::Metadata(AgentResponseMetadata {
                                    provider: "openclaw_gateway".into(),
                                    model: parsed.model.unwrap_or_else(|| provider_model.clone()),
                                    input_tokens: usage.prompt_tokens,
                                    output_tokens: usage.completion_tokens,
                                });
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

#[cfg(test)]
mod tests {
    use super::parse_stream_chunk;

    #[test]
    fn parse_stream_chunk_extracts_usage_and_model() {
        let chunk = parse_stream_chunk(
            r#"{"model":"gpt-4.1-mini","choices":[],"usage":{"prompt_tokens":321,"completion_tokens":123,"total_tokens":444}}"#,
        )
        .expect("chunk should parse");

        assert_eq!(chunk.model.as_deref(), Some("gpt-4.1-mini"));
        assert_eq!(chunk.usage.as_ref().and_then(|u| u.prompt_tokens), Some(321));
        assert_eq!(chunk.usage.as_ref().and_then(|u| u.completion_tokens), Some(123));
    }
}
