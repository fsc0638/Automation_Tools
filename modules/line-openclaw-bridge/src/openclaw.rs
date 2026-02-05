//! OpenClaw 本地 API 客戶端模組
//! 與本地運行的 OpenClaw AI 助理通訊

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{info, error};

/// OpenClaw 客戶端
pub struct OpenClawClient {
    client: Client,
    base_url: String,
    gateway_token: Option<String>,
}

/// Chat message for OpenAI-compatible API
#[derive(Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// 發送給 OpenClaw Chat Completions 的請求
#[derive(Debug, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// Chat Completions API 的回應
#[derive(Debug, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
pub struct ChatChoice {
    pub index: i32,
    pub message: ChatMessage,
    pub finish_reason: String,
}

/// OpenClaw 健康檢查回應
#[derive(Debug, Deserialize)]
pub struct HealthResponse {
    pub status: String,
}

impl OpenClawClient {
    /// 建立新的 OpenClaw 客戶端
    pub fn new(base_url: String, gateway_token: Option<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_else(|_| Client::new()),
            base_url,
            gateway_token,
        }
    }

    /// 檢查 OpenClaw 是否在線
    pub async fn health_check(&self) -> Result<bool, reqwest::Error> {
        let url = format!("{}/health", self.base_url);
        
        let response = self.client
            .get(&url)
            .send()
            .await?;
        
        Ok(response.status().is_success())
    }

    /// 發送訊息給 OpenClaw 並取得回應
    /// 使用 OpenAI-compatible Chat Completions API
    pub async fn send_message(&self, user_id: &str, message: &str) -> Result<String, String> {
        info!("Sending message to OpenClaw: user={}, message={}", user_id, message);
        
        let url = format!("{}/v1/chat/completions", self.base_url);
        
        // 構建 Chat Completions 請求
        let request = ChatCompletionRequest {
            model: "google-antigravity/claude-opus-4-5-thinking".to_string(),
            messages: vec![
                ChatMessage {
                    role: "user".to_string(),
                    content: message.to_string(),
                }
            ],
            stream: Some(false),
        };
        
        // 建立請求
        let mut req_builder = self.client
            .post(&url)
            .header("Content-Type", "application/json");
        
        // 加入認證 token
        if let Some(ref token) = self.gateway_token {
            req_builder = req_builder.header("Authorization", format!("Bearer {}", token));
        }
        
        // 發送請求
        match req_builder.json(&request).send().await {
            Ok(response) if response.status().is_success() => {
                match response.json::<ChatCompletionResponse>().await {
                    Ok(chat_response) => {
                        if let Some(choice) = chat_response.choices.first() {
                            info!("Got response from OpenClaw: {}", choice.message.content);
                            return Ok(choice.message.content.clone());
                        }
                        Err("OpenClaw 回應格式錯誤：沒有選擇項".to_string())
                    }
                    Err(e) => {
                        error!("Failed to parse OpenClaw response: {}", e);
                        Err(format!("解析 OpenClaw 回應失敗: {}", e))
                    }
                }
            }
            Ok(response) => {
                let status = response.status();
                error!("OpenClaw returned error status: {}", status);
                Err(format!("OpenClaw 返回錯誤狀態: {}", status))
            }
            Err(e) => {
                error!("Failed to connect to OpenClaw: {}", e);
                Err(format!("無法連接到 OpenClaw: {}", e))
            }
        }
    }

    /// 透過 WebSocket 連接 OpenClaw（進階功能）
    /// 這是更穩定的連接方式，但需要額外的 WebSocket 處理
    pub async fn connect_websocket(&self) -> Result<(), String> {
        // TODO: 實作 WebSocket 連接
        // OpenClaw 主要使用 WebSocket 進行即時通訊
        Err("WebSocket 連接尚未實作".to_string())
    }
}

/// 簡單的回應生成器（當 OpenClaw 不可用時使用）
pub fn fallback_response(message: &str) -> String {
    if message.contains("你好") || message.to_lowercase().contains("hello") {
        "你好！我是 LINE-OpenClaw 橋接服務。目前 OpenClaw 暫時離線，請稍後再試。".to_string()
    } else if message.contains("幫助") || message.to_lowercase().contains("help") {
        "歡迎使用 LINE-OpenClaw 整合服務！\n\n可用指令：\n• 直接輸入訊息與 AI 對話\n• 輸入「狀態」查看服務狀態".to_string()
    } else if message.contains("狀態") || message.to_lowercase().contains("status") {
        "📊 服務狀態\n• LINE Bridge: ✅ 運行中\n• OpenClaw: ⏳ 連接中...".to_string()
    } else {
        format!("收到您的訊息：「{}」\n\n目前正在連接 OpenClaw，請稍候...", message)
    }
}
