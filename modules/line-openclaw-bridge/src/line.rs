//! LINE Messaging API 客戶端模組
//! 處理 LINE 訊息發送與事件解析

use hmac::{Hmac, Mac};
use sha2::Sha256;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use reqwest::Client;
use serde::{Deserialize, Serialize};

type HmacSha256 = Hmac<Sha256>;

/// LINE API 客戶端
pub struct LineClient {
    client: Client,
    channel_access_token: String,
    channel_secret: String,
}

/// LINE 訊息事件
#[derive(Debug, Deserialize)]
pub struct WebhookEvent {
    pub events: Vec<Event>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Event {
    #[serde(rename = "message")]
    Message(MessageEvent),
    #[serde(rename = "postback")]
    Postback(PostbackEvent),
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct MessageEvent {
    #[serde(rename = "replyToken")]
    pub reply_token: String,
    pub source: Source,
    pub message: Message,
}

#[derive(Debug, Deserialize)]
pub struct PostbackEvent {
    #[serde(rename = "replyToken")]
    pub reply_token: String,
    pub source: Source,
    pub postback: Postback,
}

#[derive(Debug, Deserialize)]
pub struct Source {
    #[serde(rename = "userId")]
    pub user_id: Option<String>,
    #[serde(rename = "groupId")]
    pub group_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    #[serde(rename = "type")]
    pub message_type: String,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Postback {
    pub data: String,
}

/// 發送訊息請求
#[derive(Debug, Serialize)]
pub struct ReplyMessageRequest {
    #[serde(rename = "replyToken")]
    pub reply_token: String,
    pub messages: Vec<TextMessage>,
}

#[derive(Debug, Serialize)]
pub struct PushMessageRequest {
    pub to: String,
    pub messages: Vec<TextMessage>,
}

#[derive(Debug, Serialize)]
pub struct TextMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub text: String,
}

impl TextMessage {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            message_type: "text".to_string(),
            text: text.into(),
        }
    }
}

impl LineClient {
    /// 建立新的 LINE 客戶端
    pub fn new(channel_access_token: String, channel_secret: String) -> Self {
        Self {
            client: Client::new(),
            channel_access_token,
            channel_secret,
        }
    }

    /// 驗證 LINE Webhook 簽名
    pub fn verify_signature(&self, body: &[u8], signature: &str) -> bool {
        let mut mac = match HmacSha256::new_from_slice(self.channel_secret.as_bytes()) {
            Ok(m) => m,
            Err(_) => return false,
        };
        mac.update(body);
        let result = mac.finalize();
        let expected = BASE64.encode(result.into_bytes());
        expected == signature
    }

    /// 解析 Webhook 事件
    pub fn parse_events(&self, body: &str) -> Result<WebhookEvent, serde_json::Error> {
        serde_json::from_str(body)
    }

    /// 使用 reply token 回覆訊息
    pub async fn reply_message(&self, reply_token: &str, text: &str) -> Result<(), reqwest::Error> {
        let request = ReplyMessageRequest {
            reply_token: reply_token.to_string(),
            messages: vec![TextMessage::new(text)],
        };

        self.client
            .post("https://api.line.me/v2/bot/message/reply")
            .header("Authorization", format!("Bearer {}", self.channel_access_token))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        Ok(())
    }

    /// 主動推送訊息給用戶
    pub async fn push_message(&self, user_id: &str, text: &str) -> Result<(), reqwest::Error> {
        let request = PushMessageRequest {
            to: user_id.to_string(),
            messages: vec![TextMessage::new(text)],
        };

        self.client
            .post("https://api.line.me/v2/bot/message/push")
            .header("Authorization", format!("Bearer {}", self.channel_access_token))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        Ok(())
    }
}
