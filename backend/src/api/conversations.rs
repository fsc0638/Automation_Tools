use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    agents::orchestrator::{build_project_scope, run_agent_turn, AgentMode},
    api::{
        auth::AuthUser,
        conversation_memory::{get_project_summary, load_project_history, refresh_project_summary},
        AppState,
    },
    db::models::{Conversation, Message, Project},
    error::{AppError, AppResult},
    security::context_firewall::{secure_agent_context, AgentDataPolicy},
};

#[derive(Debug, Deserialize)]
pub struct CreateConversationRequest {
    pub title: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListConversationsQuery {
    pub mode: Option<String>,
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
            get(get_conversation).delete(delete_conversation),
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
    Query(query): Query<ListConversationsQuery>,
) -> AppResult<Json<Vec<Conversation>>> {
    verify_project_access(&state, project_id, auth_user.id).await?;

    let convs: Vec<Conversation> = match normalize_mode_optional(query.mode.as_deref()) {
        Some(mode) => {
            sqlx::query_as(
                "SELECT * FROM conversations WHERE project_id = $1 AND mode = $2 ORDER BY updated_at DESC",
            )
            .bind(project_id)
            .bind(mode)
            .fetch_all(&state.db)
            .await?
        }
        None => {
            sqlx::query_as(
                "SELECT * FROM conversations WHERE project_id = $1 ORDER BY updated_at DESC",
            )
            .bind(project_id)
            .fetch_all(&state.db)
            .await?
        }
    };

    Ok(Json(convs))
}

async fn create_conversation(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
    Json(req): Json<CreateConversationRequest>,
) -> AppResult<(StatusCode, Json<Conversation>)> {
    verify_project_access(&state, project_id, auth_user.id).await?;

    let mode = normalize_mode(req.mode.as_deref());
    let title = req.title.unwrap_or_else(|| default_title_for_mode(mode));
    let conv: Conversation = sqlx::query_as(
        "INSERT INTO conversations (project_id, user_id, title, mode)
         VALUES ($1, $2, $3, $4)
         RETURNING *",
    )
    .bind(project_id)
    .bind(auth_user.id)
    .bind(&title)
    .bind(mode)
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

    let conv: Option<Conversation> =
        sqlx::query_as("SELECT * FROM conversations WHERE id = $1 AND project_id = $2")
            .bind(conv_id)
            .bind(project_id)
            .fetch_optional(&state.db)
            .await?;

    let conv = conv.ok_or_else(|| AppError::NotFound("Conversation not found".into()))?;

    let messages: Vec<Message> =
        sqlx::query_as("SELECT * FROM messages WHERE conversation_id = $1 ORDER BY created_at ASC")
            .bind(conv_id)
            .fetch_all(&state.db)
            .await?;

    Ok(Json(ConversationWithMessages {
        conversation: conv,
        messages,
    }))
}

async fn delete_conversation(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, conv_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    verify_project_access(&state, project_id, auth_user.id).await?;

    let result =
        sqlx::query("DELETE FROM conversations WHERE id = $1 AND project_id = $2 AND user_id = $3")
            .bind(conv_id)
            .bind(project_id)
            .bind(auth_user.id)
            .execute(&state.db)
            .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Conversation not found".into()));
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn send_message(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, conv_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<SendMessageRequest>,
) -> AppResult<Json<serde_json::Value>> {
    verify_project_access(&state, project_id, auth_user.id).await?;

    let project: Project = sqlx::query_as(
        "SELECT * FROM projects WHERE id = $1 AND user_can_access_project(id, $2, 'viewer')",
    )
    .bind(project_id)
    .bind(auth_user.id)
    .fetch_one(&state.db)
    .await?;
    let project_scope = build_project_scope(&project);

    let conv: Option<Conversation> =
        sqlx::query_as("SELECT * FROM conversations WHERE id = $1 AND project_id = $2")
            .bind(conv_id)
            .bind(project_id)
            .fetch_optional(&state.db)
            .await?;
    let conv = conv.ok_or_else(|| AppError::NotFound("Conversation not found".into()))?;

    // Load conversation history before saving this turn, then append the current
    // user message exactly once for the agent call.
    let history = load_project_history(&state.db, project_id).await?;
    let project_summary = get_project_summary(&state.db, project_id).await?;

    // Save user message so every turn is persisted.
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

    let mode = agent_mode_from_str(req.mode.as_deref());
    let mode_label = normalize_mode(req.mode.as_deref());
    let data_policy = AgentDataPolicy::for_mode(&mode);
    let secured_context = secure_agent_context(
        &state.db,
        auth_user.id,
        project_id,
        conv_id,
        mode_label,
        &data_policy,
        &project_scope,
        &history,
        project_summary.map(|summary| summary.summary),
        &req.content,
    )
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("context firewall failed: {}", e)))?;

    let responses = run_agent_turn(
        &state.config,
        &secured_context.project_scope,
        &secured_context.history,
        secured_context.project_summary.as_deref(),
        &secured_context.user_message,
        mode,
    )
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

    let _ = refresh_project_summary(&state.db, &state.config, &project_scope).await;

    Ok(Json(serde_json::json!({ "messages": saved_messages })))
}

fn agent_mode_from_str(mode: Option<&str>) -> AgentMode {
    match mode {
        Some("hermes") => AgentMode::HermesOnly,
        Some("debate") => AgentMode::Debate,
        _ => AgentMode::OpenClawOnly,
    }
}

fn normalize_mode_optional(mode: Option<&str>) -> Option<&'static str> {
    match mode {
        Some("hermes") => Some("hermes"),
        Some("debate") => Some("debate"),
        Some("openclaw") => Some("openclaw"),
        _ => None,
    }
}

fn normalize_mode(mode: Option<&str>) -> &'static str {
    match mode {
        Some("hermes") => "hermes",
        Some("debate") => "debate",
        _ => "openclaw",
    }
}

fn default_title_for_mode(mode: &str) -> String {
    match mode {
        "hermes" => "Hermes Conversation".into(),
        "debate" => "Debate Conversation".into(),
        _ => "OpenClaw Conversation".into(),
    }
}

async fn verify_project_access(state: &AppState, project_id: Uuid, user_id: Uuid) -> AppResult<()> {
    let exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM projects WHERE id = $1 AND user_can_access_project(id, $2, 'viewer')",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    exists
        .map(|_| ())
        .ok_or_else(|| AppError::NotFound("Project not found".into()))
}
