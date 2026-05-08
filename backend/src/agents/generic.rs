use anyhow::{anyhow, Context, Result};
use futures_util::{Stream, StreamExt};
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use std::pin::Pin;

use crate::agents::openclaw::ChatMessage;

const REPLY_TOKEN_CEILING: u32 = 1200;

#[derive(Debug, Clone)]
pub struct AgentProfileRuntime {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
    pub base_url: Option<String>,
    pub role_prompt: String,
    pub api_key: String,
}

#[derive(Debug, Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
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

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<AnthropicMessage>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    text: Option<String>,
}

#[derive(Debug, Serialize)]
struct GeminiRequest {
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    contents: Vec<GeminiContent>,
    #[serde(rename = "generationConfig")]
    generation_config: GeminiGenerationConfig,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Serialize)]
struct GeminiGenerationConfig {
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Clone)]
pub struct GenericAgentClient {
    client: Client,
    profile: AgentProfileRuntime,
}

impl GenericAgentClient {
    pub fn new(profile: AgentProfileRuntime) -> Self {
        Self { client: Client::new(), profile }
    }

    pub fn system_prompt(&self) -> String {
        let custom = self.profile.role_prompt.trim();
        format!(
            "You are {name}, an independent user-configured software-engineering agent. \
             Provider={provider}, model={model}. Default language: Traditional Chinese. \
             Keep your own technical judgment; do not impersonate OpenClaw or Hermes. \
             Ground code claims in the provided project files and cite file paths when available. \
             Be concise, practical, and explicit when evidence is insufficient.\n\n{custom}",
            name = self.profile.name,
            provider = self.profile.provider,
            model = self.profile.model,
        )
    }

    fn build_messages(&self, messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
        let mut full = vec![ChatMessage { role: "system".into(), content: self.system_prompt() }];
        full.extend(messages.into_iter().filter_map(|msg| {
            let role = match msg.role.as_str() {
                "system" | "developer" | "user" | "assistant" => msg.role,
                "openclaw" | "hermes" | "agent" => "assistant".into(),
                _ => return None,
            };
            Some(ChatMessage { role, content: msg.content })
        }));
        full
    }

    fn openai_url(&self) -> String {
        let default_base = if self.profile.provider == "openai" {
            "https://api.openai.com/v1"
        } else {
            ""
        };
        let base = self.profile.base_url.as_deref().unwrap_or(default_base).trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/chat/completions")
        }
    }

    fn with_openai_auth(&self, req: RequestBuilder) -> RequestBuilder {
        req.bearer_auth(&self.profile.api_key)
    }

    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        match self.profile.provider.as_str() {
            "openai" | "openai_compatible" => self.openai_chat(messages).await,
            "anthropic" => self.anthropic_chat(messages).await,
            "gemini" => self.gemini_chat(messages).await,
            other => Err(anyhow!("unsupported agent provider: {other}")),
        }
    }

    async fn openai_chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let body = OpenAiChatRequest {
            model: self.profile.model.clone(),
            messages: self.build_messages(messages),
            stream: false,
            max_tokens: Some(REPLY_TOKEN_CEILING),
        };
        let response = self
            .with_openai_auth(self.client.post(self.openai_url()))
            .json(&body)
            .send()
            .await
            .context("failed to send request to custom OpenAI-compatible agent")?
            .error_for_status()
            .context("custom OpenAI-compatible agent returned an error status")?
            .json::<OpenAiChatResponse>()
            .await
            .context("failed to parse custom OpenAI-compatible response")?;
        Ok(response.choices.into_iter().next().map(|c| c.message.content).unwrap_or_default())
    }

    async fn anthropic_chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let full = self.build_messages(messages);
        let mut system = String::new();
        let mut converted = Vec::new();
        for msg in full {
            if msg.role == "system" || msg.role == "developer" {
                system.push_str(&msg.content);
                system.push_str("\n\n");
            } else {
                converted.push(AnthropicMessage {
                    role: if msg.role == "assistant" { "assistant".into() } else { "user".into() },
                    content: msg.content,
                });
            }
        }
        let body = AnthropicRequest { model: self.profile.model.clone(), max_tokens: REPLY_TOKEN_CEILING, system, messages: converted };
        let url = self.profile.base_url.as_deref().unwrap_or("https://api.anthropic.com/v1/messages").to_string();
        let response = self
            .client
            .post(url)
            .header("x-api-key", &self.profile.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .context("failed to send request to Anthropic agent")?
            .error_for_status()
            .context("Anthropic agent returned an error status")?
            .json::<AnthropicResponse>()
            .await
            .context("failed to parse Anthropic response")?;
        Ok(response.content.into_iter().filter_map(|p| p.text).collect::<Vec<_>>().join(""))
    }

    async fn gemini_chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let full = self.build_messages(messages);
        let mut system_text = String::new();
        let mut contents = Vec::new();
        for msg in full {
            if msg.role == "system" || msg.role == "developer" {
                system_text.push_str(&msg.content);
                system_text.push_str("\n\n");
            } else {
                contents.push(GeminiContent {
                    role: if msg.role == "assistant" { "model".into() } else { "user".into() },
                    parts: vec![GeminiPart { text: msg.content }],
                });
            }
        }
        let system_instruction = if system_text.trim().is_empty() {
            None
        } else {
            Some(GeminiContent { role: "user".into(), parts: vec![GeminiPart { text: system_text }] })
        };
        let body = GeminiRequest { system_instruction, contents, generation_config: GeminiGenerationConfig { max_output_tokens: REPLY_TOKEN_CEILING } };
        let base = self.profile.base_url.as_deref().unwrap_or("https://generativelanguage.googleapis.com/v1beta").trim_end_matches('/');
        let url = format!("{base}/models/{}:generateContent?key={}", self.profile.model, self.profile.api_key);
        let response = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .context("failed to send request to Gemini agent")?
            .error_for_status()
            .context("Gemini agent returned an error status")?
            .json::<GeminiResponse>()
            .await
            .context("failed to parse Gemini response")?;
        let text = response
            .candidates
            .unwrap_or_default()
            .into_iter()
            .filter_map(|c| c.content)
            .flat_map(|c| c.parts.into_iter().map(|p| p.text))
            .collect::<Vec<_>>()
            .join("");
        Ok(text)
    }

    pub fn chat_stream(&self, messages: Vec<ChatMessage>) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        if matches!(self.profile.provider.as_str(), "openai" | "openai_compatible") {
            return self.openai_chat_stream(messages);
        }
        let this = self.clone();
        Box::pin(async_stream::stream! {
            match this.chat(messages).await {
                Ok(reply) => yield reply,
                Err(error) => yield format!("[{} error: {}]", this.profile.name, error),
            }
        })
    }

    fn openai_chat_stream(&self, messages: Vec<ChatMessage>) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        let client = self.client.clone();
        let url = self.openai_url();
        let key = self.profile.api_key.clone();
        let name = self.profile.name.clone();
        let body = OpenAiChatRequest {
            model: self.profile.model.clone(),
            messages: self.build_messages(messages),
            stream: true,
            max_tokens: Some(REPLY_TOKEN_CEILING),
        };
        Box::pin(async_stream::stream! {
            let response = match client.post(url).bearer_auth(key).json(&body).send().await {
                Ok(response) => match response.error_for_status() {
                    Ok(response) => response,
                    Err(error) => {
                        yield format!("[{name} error: agent returned an error status: {error}]");
                        return;
                    }
                },
                Err(error) => {
                    yield format!("[{name} error: failed to send request: {error}]");
                    return;
                }
            };

            let mut bytes = response.bytes_stream();
            let mut buffer = String::new();
            while let Some(chunk) = bytes.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        yield format!("[{name} stream error: {error}]");
                        return;
                    }
                };
                buffer.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer = buffer[newline_pos + 1..].to_string();
                    let Some(data) = line.strip_prefix("data:").map(str::trim) else { continue; };
                    if data == "[DONE]" { return; }
                    if let Ok(parsed) = serde_json::from_str::<StreamChunk>(data) {
                        if let Some(choices) = parsed.choices {
                            for choice in choices {
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
