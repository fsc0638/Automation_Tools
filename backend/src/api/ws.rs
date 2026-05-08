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
use std::collections::HashMap;
use uuid::Uuid;

use crate::{
    agents::orchestrator::{build_project_scope, run_agent_stream, AgentMode, ServerEvent},
    api::{
        auth::verify_token,
        conversation_memory::{get_project_summary, load_project_history, refresh_project_summary},
        project_index::relevant_file_context,
        AppState,
    },
    db::models::Project,
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
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized("Invalid token".into()))?;

    let conversation_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM conversations WHERE id = $1 AND project_id = $2 AND user_id = $3",
    )
    .bind(query.conversation_id)
    .bind(query.project_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    if conversation_exists.is_none() {
        return Err(AppError::NotFound("Conversation not found".into()));
    }

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, query)))
}

async fn handle_socket(socket: WebSocket, state: AppState, query: WsQuery) {
    let (mut sender, mut receiver) = socket.split();

    let project: Project = match sqlx::query_as("SELECT * FROM projects WHERE id = $1")
        .bind(query.project_id)
        .fetch_one(&state.db)
        .await
    {
        Ok(project) => project,
        Err(_) => {
            let error = ServerEvent::Error {
                message: "Project not found for agent scope".into(),
            };
            let _ = sender
                .send(WsMessage::Text(
                    serde_json::to_string(&error).unwrap_or_default().into(),
                ))
                .await;
            return;
        }
    };
    let base_project_scope = build_project_scope(&project);

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

        let ClientEvent::Message {
            content,
            file_path,
            mode,
        } = event;

        // Load previous history before saving this turn. The orchestrator appends
        // the current user message itself, so this avoids duplicating it in agent context.
        let history = load_project_history(&state.db, query.project_id)
            .await
            .unwrap_or_default();
        let project_summary = get_project_summary(&state.db, query.project_id)
            .await
            .ok()
            .flatten();

        let user_saved = sqlx::query(
            "INSERT INTO messages (conversation_id, role, content, file_path)
             VALUES ($1, 'user', $2, $3)",
        )
        .bind(query.conversation_id)
        .bind(&content)
        .bind(&file_path)
        .execute(&state.db)
        .await;

        if user_saved.is_err() {
            let error = ServerEvent::Error {
                message: "Failed to save user message".into(),
            };
            let _ = sender
                .send(WsMessage::Text(
                    serde_json::to_string(&error).unwrap_or_default().into(),
                ))
                .await;
            continue;
        }

        let _ = sqlx::query("UPDATE conversations SET updated_at = NOW() WHERE id = $1")
            .bind(query.conversation_id)
            .execute(&state.db)
            .await;

        let agent_mode = agent_mode_from_str(mode.as_deref());
        let mut project_scope = base_project_scope.clone();
        project_scope.relevant_file_context =
            relevant_file_context(&state.db, query.project_id, &content)
                .await
                .ok()
                .flatten();

        let mut stream = run_agent_stream(
            &state.config,
            &project_scope,
            &history,
            project_summary.map(|summary| summary.summary),
            &content,
            agent_mode,
        );
        let mut buffers: HashMap<String, String> = HashMap::new();

        while let Some(event) = stream.next().await {
            match &event {
                ServerEvent::Status { .. } => {}
                ServerEvent::Chunk {
                    agent,
                    content,
                    round,
                    phase,
                } => {
                    let key = event_key(agent, *round, phase.as_deref());
                    buffers.entry(key).or_default().push_str(content);
                }
                ServerEvent::Done {
                    agent,
                    round,
                    phase,
                } => {
                    let key = event_key(agent, *round, phase.as_deref());
                    if let Some(content) = buffers.remove(&key) {
                        if !content.trim().is_empty() {
                            let role = agent_role(agent);
                            let display_name = display_agent_name(agent, *round, phase.as_deref());
                            let _ = sqlx::query(
                                "INSERT INTO messages (conversation_id, role, content, agent_name)
                                 VALUES ($1, $2, $3, $4)",
                            )
                            .bind(query.conversation_id)
                            .bind(role)
                            .bind(&content)
                            .bind(&display_name)
                            .execute(&state.db)
                            .await;

                            let _ = sqlx::query(
                                "UPDATE conversations SET updated_at = NOW() WHERE id = $1",
                            )
                            .bind(query.conversation_id)
                            .execute(&state.db)
                            .await;
                        }
                    }
                }
                ServerEvent::Error { message } => {
                    let _ = sqlx::query(
                        "INSERT INTO messages (conversation_id, role, content, agent_name)
                         VALUES ($1, 'system', $2, 'System')",
                    )
                    .bind(query.conversation_id)
                    .bind(message)
                    .execute(&state.db)
                    .await;
                }
            }

            let json = serde_json::to_string(&event).unwrap_or_default();
            if sender.send(WsMessage::Text(json.into())).await.is_err() {
                return;
            }
        }

        let _ = refresh_project_summary(&state.db, &state.config, &project_scope).await;
    }
}

fn agent_mode_from_str(mode: Option<&str>) -> AgentMode {
    match mode {
        Some("hermes") => AgentMode::HermesOnly,
        Some("debate") => AgentMode::Debate,
        _ => AgentMode::OpenClawOnly,
    }
}

fn agent_role(agent: &str) -> &'static str {
    if agent.starts_with("Hermes") {
        "hermes"
    } else {
        "openclaw"
    }
}

fn event_key(agent: &str, round: Option<usize>, phase: Option<&str>) -> String {
    format!(
        "{}:{}:{}",
        agent,
        phase.unwrap_or("single"),
        round.map(|r| r.to_string()).unwrap_or_default()
    )
}

fn display_agent_name(agent: &str, round: Option<usize>, phase: Option<&str>) -> String {
    match (phase, round) {
        (Some("round"), Some(r)) => format!("{agent} · Round {r}"),
        (Some("final"), _) => format!("{agent} · Final"),
        _ => agent.to_string(),
    }
}
