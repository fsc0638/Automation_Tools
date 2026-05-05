use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::Response,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    api::{auth::verify_token, AppState},
    agents::orchestrator::{run_agent_stream, AgentMode},
    db::models::Message,
    error::AppError,
};

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub token: String,
    pub conversation_id: Uuid,
    pub project_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientEvent {
    #[serde(rename = "message")]
    Message {
        content: String,
        file_path: Option<String>,
        mode: Option<String>,
    },
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/ws/chat", get(ws_handler))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let claims = verify_token(&query.token, &state.config.jwt_secret)?;
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Unauthorized("Invalid token".into()))?;

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, user_id, query)))
}

async fn handle_socket(socket: WebSocket, state: AppState, _user_id: Uuid, query: WsQuery) {
    let (mut sender, mut receiver) = socket.split();

    while let Some(Ok(msg)) = receiver.next().await {
        let text = match msg {
            WsMessage::Text(t) => t,
            WsMessage::Close(_) => break,
            _ => continue,
        };

        let event: ClientEvent = match serde_json::from_str(&text) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let ClientEvent::Message { content, file_path, mode } = event;

        let _ = sqlx::query(
            "INSERT INTO messages (conversation_id, role, content, file_path)
             VALUES ($1, 'user', $2, $3)",
        )
        .bind(query.conversation_id)
        .bind(&content)
        .bind(&file_path)
        .execute(&state.db)
        .await;

        let history: Vec<Message> = sqlx::query_as(
            "SELECT * FROM messages WHERE conversation_id = $1 ORDER BY created_at ASC",
        )
        .bind(query.conversation_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        let agent_mode = match mode.as_deref() {
            Some("hermes") => AgentMode::HermesOnly,
            Some("debate") => AgentMode::Debate,
            _ => AgentMode::OpenClawOnly,
        };

        let mut stream = run_agent_stream(&state.config, &history, &content, agent_mode);

        while let Some(event) = stream.next().await {
            let json = serde_json::to_string(&event).unwrap_or_default();
            if sender.send(WsMessage::Text(json.into())).await.is_err() {
                return;
            }
        }
    }
}
