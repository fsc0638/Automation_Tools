//! LINE-OpenClaw Bridge
//! 連接 LINE Bot 和本地 OpenClaw AI 助理的 Rust 服務

mod line;
mod openclaw;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Router,
    Json,
};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, error, warn};

use crate::line::{LineClient, Event};
use crate::openclaw::{OpenClawClient, fallback_response};

/// 應用程式狀態
struct AppState {
    line_client: LineClient,
    openclaw_client: OpenClawClient,
}

#[tokio::main]
async fn main() {
    // 初始化環境變數
    dotenvy::dotenv().ok();
    
    // 初始化日誌
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("line_openclaw_bridge=debug".parse().unwrap())
        )
        .init();

    // 讀取設定
    let channel_access_token = std::env::var("LINE_CHANNEL_ACCESS_TOKEN")
        .expect("LINE_CHANNEL_ACCESS_TOKEN 環境變數未設定");
    let channel_secret = std::env::var("LINE_CHANNEL_SECRET")
        .expect("LINE_CHANNEL_SECRET 環境變數未設定");
    let openclaw_base_url = std::env::var("OPENCLAW_BASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:18789".to_string());
    let openclaw_gateway_token = std::env::var("OPENCLAW_GATEWAY_TOKEN").ok();
    
    let host = std::env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("SERVER_PORT").unwrap_or_else(|_| "3000".to_string());
    
    // 建立客戶端
    let line_client = LineClient::new(channel_access_token, channel_secret);
    let openclaw_client = OpenClawClient::new(openclaw_base_url.clone(), openclaw_gateway_token);
    
    let state = Arc::new(RwLock::new(AppState {
        line_client,
        openclaw_client,
    }));

    // 建立路由
    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health_check))
        .route("/callback", post(webhook_callback))
        .with_state(state);

    // 啟動伺服器
    let addr = format!("{}:{}", host, port);
    info!("🚀 LINE-OpenClaw Bridge 啟動中...");
    info!("📍 監聽地址: http://{}", addr);
    info!("📌 Webhook URL: http://your-domain:{}/callback", port);
    info!("🔗 OpenClaw: {}", openclaw_base_url);
    info!("\n💡 提示：使用 ngrok 建立公開 URL：ngrok http {}", port);
    
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// 根路徑
async fn root() -> &'static str {
    "LINE-OpenClaw Bridge Service v0.1.0"
}

/// 健康檢查端點
async fn health_check(
    State(state): State<Arc<RwLock<AppState>>>,
) -> Json<serde_json::Value> {
    let state = state.read().await;
    let openclaw_status = match state.openclaw_client.health_check().await {
        Ok(true) => "online",
        Ok(false) => "offline",
        Err(_) => "unreachable",
    };
    
    Json(json!({
        "status": "ok",
        "service": "line-openclaw-bridge",
        "openclaw": openclaw_status
    }))
}

/// LINE Webhook 回調端點
async fn webhook_callback(
    State(state): State<Arc<RwLock<AppState>>>,
    headers: HeaderMap,
    body: String,
) -> Result<&'static str, StatusCode> {
    // 取得簽名
    let signature = headers
        .get("x-line-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            warn!("Missing X-Line-Signature header");
            StatusCode::BAD_REQUEST
        })?;

    let state_guard = state.read().await;
    
    // 驗證簽名
    if !state_guard.line_client.verify_signature(body.as_bytes(), signature) {
        error!("Invalid signature");
        return Err(StatusCode::UNAUTHORIZED);
    }
    
    // 解析事件
    let webhook_event = state_guard.line_client.parse_events(&body)
        .map_err(|e| {
            error!("Failed to parse webhook event: {}", e);
            StatusCode::BAD_REQUEST
        })?;

    info!("Received {} events", webhook_event.events.len());
    
    // 處理每個事件
    for event in webhook_event.events {
        match event {
            Event::Message(msg_event) => {
                if let Some(text) = &msg_event.message.text {
                    info!("Text message: {}", text);
                    
                    let user_id = msg_event.source.user_id.clone().unwrap_or_default();
                    
                    // 嘗試發送給 OpenClaw
                    let response = match state_guard.openclaw_client.send_message(&user_id, text).await {
                        Ok(resp) => resp,
                        Err(e) => {
                            warn!("OpenClaw error: {}", e);
                            fallback_response(text)
                        }
                    };
                    
                    // 回覆 LINE
                    if let Err(e) = state_guard.line_client.reply_message(&msg_event.reply_token, &response).await {
                        error!("Failed to reply: {}", e);
                    }
                }
            }
            Event::Postback(pb_event) => {
                info!("Postback: {}", pb_event.postback.data);
                
                let user_id = pb_event.source.user_id.clone().unwrap_or_default();
                let response = match state_guard.openclaw_client.send_message(&user_id, &pb_event.postback.data).await {
                    Ok(resp) => resp,
                    Err(e) => {
                        warn!("OpenClaw error: {}", e);
                        format!("收到按鈕點擊：{}", pb_event.postback.data)
                    }
                };
                
                if let Err(e) = state_guard.line_client.reply_message(&pb_event.reply_token, &response).await {
                    error!("Failed to reply: {}", e);
                }
            }
            Event::Unknown => {
                info!("Unknown event type, skipping");
            }
        }
    }
    
    Ok("OK")
}
