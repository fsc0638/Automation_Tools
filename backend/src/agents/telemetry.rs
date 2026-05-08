use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentResponseMetadata {
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStreamEvent {
    Content(String),
    Metadata(AgentResponseMetadata),
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct OpenAiUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct OpenAiStreamDelta {
    pub content: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct OpenAiStreamChoice {
    pub delta: OpenAiStreamDelta,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct OpenAiStreamChunk {
    pub choices: Option<Vec<OpenAiStreamChoice>>,
    pub model: Option<String>,
    pub usage: Option<OpenAiUsage>,
}

pub fn parse_openai_stream_chunk(data: &str) -> serde_json::Result<OpenAiStreamChunk> {
    serde_json::from_str::<OpenAiStreamChunk>(data)
}
