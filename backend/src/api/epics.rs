//! User-scoped epics (B3). An epic is a milestone bucket that can group
//! tasks across multiple projects — unlike `sprints`, which are
//! project-scoped. project_tasks.epic_id is the binding.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, patch as http_patch},
    Extension, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    error::{AppError, AppResult},
};

#[derive(Debug, Serialize, FromRow)]
pub struct Epic {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub status: String,
    pub target_date: Option<NaiveDate>,
    pub task_total: i64,
    pub task_done: i64,
    /// Count of distinct projects that have at least one task in this
    /// epic. Surfaces "how cross-project is this epic really?" without
    /// the client fetching the task list.
    pub project_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEpic {
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub status: Option<String>,
    pub target_date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateEpic {
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub status: Option<String>,
    pub target_date: Option<NaiveDate>,
}

const EPIC_SELECT: &str = "SELECT e.id, e.user_id, e.name, e.description,
        e.color, e.status, e.target_date,
        COALESCE(s.total, 0)         AS task_total,
        COALESCE(s.done, 0)          AS task_done,
        COALESCE(s.project_count, 0) AS project_count,
        e.created_at, e.updated_at
     FROM epics e
     LEFT JOIN (
        SELECT epic_id,
               COUNT(*)::int8                                AS total,
               COUNT(*) FILTER (WHERE status = 'done')::int8 AS done,
               COUNT(DISTINCT project_id)::int8              AS project_count
        FROM project_tasks
        WHERE epic_id IS NOT NULL
        GROUP BY epic_id
     ) s ON s.epic_id = e.id";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/epics", get(list_epics).post(create_epic))
        .route("/epics/:epic_id", http_patch(update_epic).delete(delete_epic))
}

fn valid_status(s: &str) -> bool {
    matches!(s, "planned" | "active" | "done" | "archived")
}

async fn list_epics(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> AppResult<Json<Vec<Epic>>> {
    let sql = format!(
        "{EPIC_SELECT}
         WHERE e.user_id = $1
         ORDER BY
            CASE e.status WHEN 'active' THEN 0 WHEN 'planned' THEN 1
                          WHEN 'done' THEN 2 ELSE 3 END,
            e.created_at DESC"
    );
    let epics: Vec<Epic> = sqlx::query_as(&sql)
        .bind(auth_user.id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(epics))
}

async fn create_epic(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateEpic>,
) -> AppResult<(StatusCode, Json<Epic>)> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }
    let status = req.status.as_deref().unwrap_or("planned");
    if !valid_status(status) {
        return Err(AppError::BadRequest("invalid status".into()));
    }
    let new_id: (Uuid,) = sqlx::query_as(
        "INSERT INTO epics (user_id, name, description, color, status, target_date)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(auth_user.id)
    .bind(name)
    .bind(req.description.as_deref())
    .bind(req.color.as_deref())
    .bind(status)
    .bind(req.target_date)
    .fetch_one(&state.db)
    .await?;

    let sql = format!("{EPIC_SELECT} WHERE e.id = $1");
    let epic: Epic = sqlx::query_as(&sql)
        .bind(new_id.0)
        .fetch_one(&state.db)
        .await?;
    Ok((StatusCode::CREATED, Json(epic)))
}

async fn update_epic(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(epic_id): Path<Uuid>,
    Json(req): Json<UpdateEpic>,
) -> AppResult<Json<Epic>> {
    if let Some(s) = req.status.as_deref() {
        if !valid_status(s) {
            return Err(AppError::BadRequest("invalid status".into()));
        }
    }
    let updated: Option<(Uuid,)> = sqlx::query_as(
        "UPDATE epics SET
            name        = COALESCE($1, name),
            description = COALESCE($2, description),
            color       = COALESCE($3, color),
            status      = COALESCE($4, status),
            target_date = COALESCE($5, target_date),
            updated_at  = NOW()
         WHERE id = $6 AND user_id = $7
         RETURNING id",
    )
    .bind(req.name.as_deref().map(str::trim))
    .bind(req.description.as_deref())
    .bind(req.color.as_deref())
    .bind(req.status.as_deref())
    .bind(req.target_date)
    .bind(epic_id)
    .bind(auth_user.id)
    .fetch_optional(&state.db)
    .await?;
    let id = updated.ok_or_else(|| AppError::NotFound("Epic not found".into()))?.0;
    let sql = format!("{EPIC_SELECT} WHERE e.id = $1");
    let epic: Epic = sqlx::query_as(&sql)
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(epic))
}

async fn delete_epic(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(epic_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let result = sqlx::query("DELETE FROM epics WHERE id = $1 AND user_id = $2")
        .bind(epic_id)
        .bind(auth_user.id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Epic not found".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}
