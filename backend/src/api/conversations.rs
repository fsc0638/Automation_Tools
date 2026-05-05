use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    db::models::{Conversation, Message},
    error::{AppError, AppResult},
};

#[derive(Debug, Deserialize)]
pub struct CreateConversationRequest {
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    pub file_path: Option<String>,
    pub mode: Option<String>, // "hermes" | "openclaw" | "debate"
}

#[derive(Debug, Serialize)]
pub struct ConversationWithMessages {
    #[serde(flatten)]
    pub conversation: Conversation,
    pub messages: Vec<Message>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/:project_id/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route(
            "/projects/:project_id/conversations/:conv_id",
            get(get_conversation),
        )
        .route(
            "/projects/:project_id/conversations/:conv_id/messages",
            post(send_message),
        )
}

async fn list_conversations(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
) -> AppResult<Json<Vec<Conversation>>> {
    verify_project_access(&state, project_id, auth_user.id).await?;

    let convs: Vec<Conversation> = sqlx::query_as(
        "SELECT * FROM conversations WHERE project_id = $1 ORDER BY updated_at DESC",
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(convs))
}

async fn create_conversation(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
    Json(req): Json<CreateConversationRequest>,
) -> AppResult<(StatusCode, Json<Conversation>)> {
    verify_project_access(&state, project_id, auth_user.id).await?;

    let title = req.title.unwrap_or_else(|| "New Conversation".into());
    let conv: Conversation = sqlx::query_as(
        "INSERT INTO conversations (project_id, user_id, title)
         VALUES ($1, $2, $3)
         RETURNING *",
    )
    .bind(project_id)
    .bind(auth_user.id)
    .bind(&title)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(conv)))
}

async fn get_conversation(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, conv_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<ConversationWithMessages>> {
    verify_project_access(&state, project_id, auth_user.id).await?;

    let conv: Option<Conversation> = sqlx::query_as(
        "SELECT * FROM conversations WHERE id = $1 AND project_id = $2",
    )
    .bind(conv_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;

    let conv = conv.ok_or_else(|| AppError::NotFound("Conversation not found".into()))?;

    let messages: Vec<Message> = sqlx::query_as(
        "SELECT * FROM messages WHERE conversation_id = $1 ORDER BY created_at ASC",
    )
    .bind(conv_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(ConversationWithMessages { conversation: conv, messages }))
}

async fn send_message(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, conv_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<SendMessageRequest>,
) -> AppResult<Json<serde_json::Value>> {
    use crate::agents::orchestrator::{run_agent_turn, AgentMode};

    verify_project_access(&state, project_id, auth_user.id).await?;

    let conv: Option<Conversation> = sqlx::query_as(
        "SELECT * FROM conversations WHERE id = $1 AND project_id = $2",
    )
    .bind(conv_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;
    let conv = conv.ok_or_else(|| AppError::NotFound("Conversation not found".into()))?;

    // Save user message
    let _user_msg: Message = sqlx::query_as(
        "INSERT INTO messages (conversation_id, role, content, file_path)
         VALUES ($1, 'user', $2, $3)
         RETURNING *",
    )
    .bind(conv.id)
    .bind(&req.content)
    .bind(&req.file_path)
    .fetch_one(&state.db)
    .await?;

    // Load conversation history
    let history: Vec<Message> = sqlx::query_as(
        "SELECT * FROM messages WHERE conversation_id = $1 ORDER BY created_at ASC",
    )
    .bind(conv_id)
    .fetch_all(&state.db)
    .await?;

    let mode = match req.mode.as_deref() {
        Some("hermes") => AgentMode::HermesOnly,
        Some("openclaw") => AgentMode::OpenClawOnly,
        Some("debate") => AgentMode::Debate,
        _ => AgentMode::OpenClawOnly,
    };

    let responses = run_agent_turn(&state.config, &history, &req.content, mode)
        .await
        .map_err(|e| AppError::Agent(e.to_string()))?;

    let mut saved_messages = vec![];
    for (role, content, agent_name) in &responses {
        let msg: Message = sqlx::query_as(
            "INSERT INTO messages (conversation_id, role, content, agent_name)
             VALUES ($1, $2, $3, $4)
             RETURNING *",
        )
        .bind(conv_id)
        .bind(role)
        .bind(content)
        .bind(agent_name)
        .fetch_one(&state.db)
        .await?;
        saved_messages.push(msg);
    }

    sqlx::query("UPDATE conversations SET updated_at = NOW() WHERE id = $1")
        .bind(conv_id)
        .execute(&state.db)
        .await?;

    Ok(Json(serde_json::json!({ "messages": saved_messages })))
}

async fn verify_project_access(
    state: &AppState,
    project_id: Uuid,
    user_id: Uuid,
) -> AppResult<()> {
    let exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM projects WHERE id = $1 AND user_id = $2")
            .bind(project_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;

    exists
        .map(|_| ())
        .ok_or_else(|| AppError::NotFound("Project not found".into()))
}
