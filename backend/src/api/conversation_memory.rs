use anyhow::Result as AnyResult;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Extension, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    agents::{
        openclaw::{ChatMessage, OpenClawClient},
        orchestrator::ProjectScope,
    },
    api::{auth::AuthUser, AppState},
    config::Config,
    db::models::{Message, ProjectMemorySummary},
    error::{AppError, AppResult},
    security::redaction::redact_secrets,
};

const PROJECT_HISTORY_LIMIT: i64 = 120;
const SUMMARY_SOURCE_LIMIT: i64 = 40;
const SUMMARY_CHAR_BUDGET: usize = 14_000;
const SUMMARY_MAX_CHARS: usize = 2_400;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ProjectMemoryCandidate {
    pub id: Uuid,
    pub project_id: Uuid,
    pub candidate_type: String,
    pub proposed_content: String,
    pub source_message_count: i32,
    pub source_context_hash: String,
    pub status: String,
    pub review_note: Option<String>,
    pub reviewed_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub applied_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct CandidateQuery {
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ReviewCandidateRequest {
    pub review_note: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/:project_id/memory/candidates",
            get(list_memory_candidates),
        )
        .route(
            "/projects/:project_id/memory/candidates/:candidate_id/approve",
            post(approve_memory_candidate),
        )
        .route(
            "/projects/:project_id/memory/candidates/:candidate_id/reject",
            post(reject_memory_candidate),
        )
}

pub async fn load_project_history(
    db: &PgPool,
    project_id: Uuid,
) -> Result<Vec<Message>, sqlx::Error> {
    let mut rows: Vec<Message> = sqlx::query_as(
        "SELECT m.*
         FROM messages m
         INNER JOIN conversations c ON c.id = m.conversation_id
         WHERE c.project_id = $1
         ORDER BY m.created_at DESC
         LIMIT $2",
    )
    .bind(project_id)
    .bind(PROJECT_HISTORY_LIMIT)
    .fetch_all(db)
    .await?;

    rows.reverse();
    Ok(rows)
}

pub async fn get_project_summary(
    db: &PgPool,
    project_id: Uuid,
) -> Result<Option<ProjectMemorySummary>, sqlx::Error> {
    sqlx::query_as(
        "SELECT project_id, summary, source_message_count, updated_at
         FROM project_memory_summaries
         WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(db)
    .await
}

pub async fn refresh_project_summary(
    db: &PgPool,
    config: &Arc<Config>,
    project: &ProjectScope,
) -> AnyResult<Option<ProjectMemorySummary>> {
    let recent_messages = load_recent_project_messages_for_summary(db, project.id).await?;
    if recent_messages.is_empty() {
        return Ok(None);
    }

    let existing = get_project_summary(db, project.id).await?;
    let prompt = build_summary_prompt(project, existing.as_ref(), &recent_messages);
    let summary = OpenClawClient::new(config).chat(prompt).await?;
    let normalized = normalize_summary(&summary);
    if normalized.is_empty() {
        return Ok(existing);
    }

    if existing.as_ref().map(|s| s.summary.trim()) == Some(normalized.trim()) {
        return Ok(existing);
    }

    let source_message_count = recent_messages.len() as i32;
    let source_context_hash = memory_candidate_hash(project.id, &normalized, source_message_count);
    let pending_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM project_memory_candidates
         WHERE project_id = $1 AND source_context_hash = $2 AND status = 'pending'
         LIMIT 1",
    )
    .bind(project.id)
    .bind(&source_context_hash)
    .fetch_optional(db)
    .await?;

    if pending_exists.is_none() {
        sqlx::query(
            "INSERT INTO project_memory_candidates
             (project_id, candidate_type, proposed_content, source_message_count, source_context_hash)
             VALUES ($1, 'project_summary', $2, $3, $4)",
        )
        .bind(project.id)
        .bind(&normalized)
        .bind(source_message_count)
        .bind(&source_context_hash)
        .execute(db)
        .await?;
    }

    Ok(existing)
}

async fn list_memory_candidates(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
    Query(query): Query<CandidateQuery>,
) -> AppResult<Json<Vec<ProjectMemoryCandidate>>> {
    verify_project_access(&state.db, project_id, auth_user.id).await?;
    let status = normalize_candidate_status(query.status.as_deref())?;

    let rows: Vec<ProjectMemoryCandidate> = sqlx::query_as(
        "SELECT * FROM project_memory_candidates
         WHERE project_id = $1
           AND ($2::text IS NULL OR status = $2)
         ORDER BY created_at DESC
         LIMIT 100",
    )
    .bind(project_id)
    .bind(status)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}

async fn approve_memory_candidate(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, candidate_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<ReviewCandidateRequest>,
) -> AppResult<Json<ProjectMemoryCandidate>> {
    verify_project_access(&state.db, project_id, auth_user.id).await?;

    let candidate: ProjectMemoryCandidate = sqlx::query_as(
        "SELECT * FROM project_memory_candidates
         WHERE id = $1 AND project_id = $2 AND status = 'pending'",
    )
    .bind(candidate_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Pending memory candidate not found".into()))?;

    sqlx::query(
        "INSERT INTO project_memory_summaries (project_id, summary, source_message_count, updated_at)
         VALUES ($1, $2, $3, NOW())
         ON CONFLICT (project_id)
         DO UPDATE SET summary = EXCLUDED.summary,
                       source_message_count = EXCLUDED.source_message_count,
                       updated_at = NOW()",
    )
    .bind(project_id)
    .bind(&candidate.proposed_content)
    .bind(candidate.source_message_count)
    .execute(&state.db)
    .await?;

    let updated: ProjectMemoryCandidate = sqlx::query_as(
        "UPDATE project_memory_candidates
         SET status = 'approved', review_note = $3, reviewed_by = $4,
             reviewed_at = NOW(), applied_at = NOW()
         WHERE id = $1 AND project_id = $2
         RETURNING *",
    )
    .bind(candidate_id)
    .bind(project_id)
    .bind(clean_optional(req.review_note.as_deref()))
    .bind(auth_user.id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(updated))
}

async fn reject_memory_candidate(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, candidate_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<ReviewCandidateRequest>,
) -> AppResult<StatusCode> {
    verify_project_access(&state.db, project_id, auth_user.id).await?;
    let result = sqlx::query(
        "UPDATE project_memory_candidates
         SET status = 'rejected', review_note = $3, reviewed_by = $4, reviewed_at = NOW()
         WHERE id = $1 AND project_id = $2 AND status = 'pending'",
    )
    .bind(candidate_id)
    .bind(project_id)
    .bind(clean_optional(req.review_note.as_deref()))
    .bind(auth_user.id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(
            "Pending memory candidate not found".into(),
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn load_recent_project_messages_for_summary(
    db: &PgPool,
    project_id: Uuid,
) -> Result<Vec<Message>, sqlx::Error> {
    let mut rows: Vec<Message> = sqlx::query_as(
        "SELECT m.*
         FROM messages m
         INNER JOIN conversations c ON c.id = m.conversation_id
         WHERE c.project_id = $1
         ORDER BY m.created_at DESC
         LIMIT $2",
    )
    .bind(project_id)
    .bind(SUMMARY_SOURCE_LIMIT)
    .fetch_all(db)
    .await?;

    rows.reverse();
    Ok(rows)
}

fn build_summary_prompt(
    project: &ProjectScope,
    existing: Option<&ProjectMemorySummary>,
    recent_messages: &[Message],
) -> Vec<ChatMessage> {
    let existing_summary_raw = existing
        .map(|s| s.summary.as_str())
        .unwrap_or("[No previous summary]");
    let existing_summary = redact_secrets(existing_summary_raw).text;
    let root = project.root.as_deref().unwrap_or("unknown");
    let recent_transcript = render_recent_messages(recent_messages);

    vec![
        ChatMessage {
            role: "system".into(),
            content: "You are OpenClaw maintaining durable project memory for a multi-agent coding workspace. Respond in Traditional Chinese. Produce a concise but information-dense markdown summary that future Hermes/OpenClaw turns can load as project memory. Do not distort, invent, or promote unresolved debate into consensus. Focus only on durable facts grounded in the transcript: architecture, confirmed decisions, accepted constraints, important file paths, unresolved questions, user preferences, and recent meaningful changes. Exclude chatter, duplicated reasoning, and ephemeral phrasing. If facts conflict, call out the conflict explicitly instead of guessing. Keep the summary under 12 bullets and under 2400 characters.".into(),
        },
        ChatMessage {
            role: "user".into(),
            content: format!(
                "Project id: {}\nProject name: {}\nProject root: {}\n\nExisting project memory summary:\n{}\n\nRecent cross-conversation transcript excerpt:\n{}\n\nRewrite the project memory summary for future turns. Use this structure:\n## Current state\n- ...\n## Stable decisions / constraints\n- ...\n## Open questions / risks\n- ...\n## Recent notable changes\n- ...\nOnly include bullets that are grounded in the provided transcript or project metadata.",
                project.id,
                project.name,
                root,
                existing_summary,
                recent_transcript,
            ),
        },
    ]
}

fn render_recent_messages(messages: &[Message]) -> String {
    let mut output = String::new();
    for message in messages {
        let label = match message.role.as_str() {
            "user" => "User",
            "hermes" => message.agent_name.as_deref().unwrap_or("Hermes"),
            "openclaw" => message.agent_name.as_deref().unwrap_or("OpenClaw"),
            "system" => message.agent_name.as_deref().unwrap_or("System"),
            _ => "Unknown",
        };
        let sanitized = redact_secrets(&message.content.replace("\r", "").replace("\n", " ")).text;
        output.push_str("- ");
        output.push_str(label);
        output.push_str(": ");
        output.push_str(&sanitized);
        output.push('\n');
        if output.len() >= SUMMARY_CHAR_BUDGET {
            output.truncate(SUMMARY_CHAR_BUDGET);
            output.push_str("\n...[truncated]");
            break;
        }
    }
    output
}

fn normalize_summary(summary: &str) -> String {
    let normalized = summary.replace("\r\n", "\n").trim().to_string();
    if normalized.chars().count() <= SUMMARY_MAX_CHARS {
        normalized
    } else {
        normalized
            .chars()
            .take(SUMMARY_MAX_CHARS)
            .collect::<String>()
            .trim()
            .to_string()
    }
}

fn memory_candidate_hash(project_id: Uuid, content: &str, source_message_count: i32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(project_id.as_bytes());
    hasher.update(source_message_count.to_le_bytes());
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

async fn verify_project_access(db: &PgPool, project_id: Uuid, user_id: Uuid) -> AppResult<()> {
    let exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM projects WHERE id = $1 AND user_can_access_project(id, $2, 'viewer')",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;

    exists
        .map(|_| ())
        .ok_or_else(|| AppError::NotFound("Project not found".into()))
}

fn normalize_candidate_status(value: Option<&str>) -> AppResult<Option<&'static str>> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None | Some("all") => Ok(None),
        Some("pending") => Ok(Some("pending")),
        Some("approved") => Ok(Some("approved")),
        Some("rejected") => Ok(Some("rejected")),
        _ => Err(AppError::BadRequest(
            "status must be pending, approved, rejected, or all".into(),
        )),
    }
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}
