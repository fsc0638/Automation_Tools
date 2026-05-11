//! User-scoped shared memory notes (B6). Each note is a pinned
//! fact / decision that can opt into being visible to one or more of
//! the user's projects (or all of them when scope_projects is empty).
//! Project chat can pull these into agent context to carry decisions
//! across project boundaries.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, patch as http_patch},
    Extension, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    error::{AppError, AppResult},
};

#[derive(Debug, Serialize, FromRow)]
pub struct SharedMemoryNote {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    /// Empty array = note is visible globally to every project the user
    /// owns. Non-empty = whitelist of project ids opted in.
    pub scope_projects: Vec<Uuid>,
    pub pinned: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateNote {
    pub title: String,
    pub body: String,
    pub tags: Option<Vec<String>>,
    pub scope_projects: Option<Vec<Uuid>>,
    pub pinned: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNote {
    pub title: Option<String>,
    pub body: Option<String>,
    pub tags: Option<Vec<String>>,
    pub scope_projects: Option<Vec<Uuid>>,
    pub pinned: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ListNoteQuery {
    /// Filter to notes that apply to this project (either global or
    /// scoped to it). Omit to list all of the user's notes.
    pub project_id: Option<Uuid>,
    /// Substring match (case-insensitive) against title / body / tags.
    pub q: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/memory", get(list_notes).post(create_note))
        .route("/memory/:note_id", http_patch(update_note).delete(delete_note))
}

async fn list_notes(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(query): Query<ListNoteQuery>,
) -> AppResult<Json<Vec<SharedMemoryNote>>> {
    // Pinned first, then most-recently-updated. project_id filter uses
    // PG array semantics: empty scope_projects → applies everywhere;
    // non-empty → applies if the array contains the target project.
    let q_pattern = query
        .q
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", s.to_lowercase()));

    let notes: Vec<SharedMemoryNote> = sqlx::query_as(
        "SELECT * FROM shared_memory_notes
         WHERE user_id = $1
           AND (
             $2::uuid IS NULL
             OR cardinality(scope_projects) = 0
             OR $2::uuid = ANY(scope_projects)
           )
           AND (
             $3::text IS NULL
             OR LOWER(title) LIKE $3
             OR LOWER(body)  LIKE $3
             OR EXISTS (SELECT 1 FROM unnest(tags) tag WHERE LOWER(tag) LIKE $3)
           )
         ORDER BY pinned DESC, updated_at DESC",
    )
    .bind(auth_user.id)
    .bind(query.project_id)
    .bind(q_pattern)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(notes))
}

async fn create_note(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateNote>,
) -> AppResult<(StatusCode, Json<SharedMemoryNote>)> {
    let title = req.title.trim();
    let body = req.body.trim();
    if title.is_empty() || body.is_empty() {
        return Err(AppError::BadRequest("title and body are required".into()));
    }
    let tags = req.tags.unwrap_or_default();
    let scope = req.scope_projects.unwrap_or_default();

    let note: SharedMemoryNote = sqlx::query_as(
        "INSERT INTO shared_memory_notes (user_id, title, body, tags, scope_projects, pinned)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
    )
    .bind(auth_user.id)
    .bind(title)
    .bind(body)
    .bind(&tags)
    .bind(&scope)
    .bind(req.pinned.unwrap_or(false))
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(note)))
}

async fn update_note(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(note_id): Path<Uuid>,
    Json(req): Json<UpdateNote>,
) -> AppResult<Json<SharedMemoryNote>> {
    let note: Option<SharedMemoryNote> = sqlx::query_as(
        "UPDATE shared_memory_notes SET
            title          = COALESCE($1, title),
            body           = COALESCE($2, body),
            tags           = COALESCE($3, tags),
            scope_projects = COALESCE($4, scope_projects),
            pinned         = COALESCE($5, pinned),
            updated_at     = NOW()
         WHERE id = $6 AND user_id = $7
         RETURNING *",
    )
    .bind(req.title.as_deref().map(str::trim))
    .bind(req.body.as_deref().map(str::trim))
    .bind(req.tags.as_deref())
    .bind(req.scope_projects.as_deref())
    .bind(req.pinned)
    .bind(note_id)
    .bind(auth_user.id)
    .fetch_optional(&state.db)
    .await?;
    note.map(Json).ok_or_else(|| AppError::NotFound("Note not found".into()))
}

async fn delete_note(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(note_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let result = sqlx::query(
        "DELETE FROM shared_memory_notes WHERE id = $1 AND user_id = $2",
    )
    .bind(note_id)
    .bind(auth_user.id)
    .execute(&state.db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Note not found".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}
