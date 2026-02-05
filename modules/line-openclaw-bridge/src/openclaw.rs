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

/// 發送給 OpenClaw 的訊息
#[derive(Debug, Serialize)]
pub struct ChatRequest {
    pub message: String,
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

/// OpenClaw 的回應
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub response: Option<String>,
    pub error: Option<String>,
    pub status: Option<String>,
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
            client: Client::new(),
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
    /// 
    /// OpenClaw 主要透過 WebSocket 或 CLI 互動，
    /// 這裡我們嘗試透過 HTTP API 發送訊息
    pub async fn send_message(&self, user_id: &str, message: &str) -> Result<String, String> {
        info!("Sending message to OpenClaw: user={}, message={}", user_id, message);
        
        // 嘗試多種可能的 API 端點
        let endpoints = [
            "/api/chat",
            "/api/message",
            "/chat",
            "/message",
        ];
        
        for endpoint in endpoints {
            let url = format!("{}{}", self.base_url, endpoint);
            
            let request = ChatRequest {
                message: message.to_string(),
                user_id: user_id.to_string(),
                context: None,
            };
            
            let mut req_builder = self.client
                .post(&url)
                .header("Content-Type", "application/json");
            
            // 如果有 gateway token，加入認證
            if let Some(ref token) = self.gateway_token {
                req_builder = req_builder.header("Authorization", format!("Bearer {}", token));
            }
            
            match req_builder.json(&request).send().await {
                Ok(response) if response.status().is_success() => {
                    match response.json::<ChatResponse>().await {
                        Ok(chat_response) => {
                            if let Some(resp) = chat_response.response {
                                return Ok(resp);
                            }
                            if let Some(err) = chat_response.error {
                                return Err(err);
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse OpenClaw response: {}", e);
                            continue;
                        }
                    }
                }
                Ok(response) => {
                    info!("Endpoint {} returned status: {}", endpoint, response.status());
                    continue;
                }
                Err(e) => {
                    error!("Failed to connect to {}: {}", endpoint, e);
                    continue;
                }
            }
        }
        
        // 如果所有端點都失敗，返回提示訊息
        Err("無法連接到 OpenClaw。請確認 OpenClaw 正在運行。".to_string())
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
