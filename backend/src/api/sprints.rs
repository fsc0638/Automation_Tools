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
pub struct Sprint {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub goal: Option<String>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Count of tasks linked to this sprint, totaled by status. Populated
    /// by the SELECT JOIN so the Roadmap can show "5 done / 12 total".
    pub task_total: i64,
    pub task_done: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateSprint {
    pub name: String,
    pub goal: Option<String>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSprint {
    pub name: Option<String>,
    pub goal: Option<String>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub status: Option<String>,
}

const SPRINT_SELECT: &str = "SELECT s.id, s.project_id, s.name, s.goal,
        s.start_date, s.end_date, s.status, s.created_at, s.updated_at,
        COALESCE(t.total, 0) AS task_total,
        COALESCE(t.done, 0)  AS task_done
     FROM sprints s
     LEFT JOIN (
        SELECT sprint_id,
               COUNT(*) AS total,
               COUNT(*) FILTER (WHERE status = 'done') AS done
        FROM project_tasks
        WHERE sprint_id IS NOT NULL
        GROUP BY sprint_id
     ) t ON t.sprint_id = s.id";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/projects/:id/sprints", get(list_sprints).post(create_sprint))
        .route(
            "/projects/:id/sprints/:sprint_id",
            http_patch(update_sprint).delete(delete_sprint),
        )
}

async fn verify_access(state: &AppState, project_id: Uuid, user_id: Uuid) -> AppResult<()> {
    let exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM projects WHERE id = $1 AND user_can_access_project(id, $2, 'viewer')",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("Project not found".into()));
    }
    Ok(())
}

fn valid_status(s: &str) -> bool {
    matches!(s, "planned" | "active" | "closed")
}

async fn list_sprints(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
) -> AppResult<Json<Vec<Sprint>>> {
    verify_access(&state, project_id, auth_user.id).await?;
    // Active sprints first, then planned, then closed. Within a status,
    // newest first so recent work surfaces ahead of historical sprints.
    let sql = format!(
        "{SPRINT_SELECT}
         WHERE s.project_id = $1
         ORDER BY
            CASE s.status WHEN 'active' THEN 0 WHEN 'planned' THEN 1 ELSE 2 END,
            s.created_at DESC"
    );
    let sprints: Vec<Sprint> = sqlx::query_as(&sql)
        .bind(project_id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(sprints))
}

async fn create_sprint(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
    Json(req): Json<CreateSprint>,
) -> AppResult<(StatusCode, Json<Sprint>)> {
    verify_access(&state, project_id, auth_user.id).await?;
    let name = req.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }
    let status = req.status.as_deref().unwrap_or("planned");
    if !valid_status(status) {
        return Err(AppError::BadRequest("invalid status".into()));
    }

    let new_id: (Uuid,) = sqlx::query_as(
        "INSERT INTO sprints (project_id, name, goal, start_date, end_date, status)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(project_id)
    .bind(name)
    .bind(req.goal.as_deref())
    .bind(req.start_date)
    .bind(req.end_date)
    .bind(status)
    .fetch_one(&state.db)
    .await?;

    let sql = format!("{SPRINT_SELECT} WHERE s.id = $1");
    let sprint: Sprint = sqlx::query_as(&sql)
        .bind(new_id.0)
        .fetch_one(&state.db)
        .await?;
    Ok((StatusCode::CREATED, Json(sprint)))
}

async fn update_sprint(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, sprint_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateSprint>,
) -> AppResult<Json<Sprint>> {
    verify_access(&state, project_id, auth_user.id).await?;
    if let Some(s) = req.status.as_deref() {
        if !valid_status(s) {
            return Err(AppError::BadRequest("invalid status".into()));
        }
    }
    let updated: Option<(Uuid,)> = sqlx::query_as(
        "UPDATE sprints SET
            name       = COALESCE($1, name),
            goal       = COALESCE($2, goal),
            start_date = COALESCE($3, start_date),
            end_date   = COALESCE($4, end_date),
            status     = COALESCE($5, status),
            updated_at = NOW()
         WHERE id = $6 AND project_id = $7
         RETURNING id",
    )
    .bind(req.name.as_deref().map(str::trim))
    .bind(req.goal.as_deref())
    .bind(req.start_date)
    .bind(req.end_date)
    .bind(req.status.as_deref())
    .bind(sprint_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;
    let id = updated.ok_or_else(|| AppError::NotFound("Sprint not found".into()))?.0;
    let sql = format!("{SPRINT_SELECT} WHERE s.id = $1");
    let sprint: Sprint = sqlx::query_as(&sql)
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(sprint))
}

async fn delete_sprint(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, sprint_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    verify_access(&state, project_id, auth_user.id).await?;
    let result = sqlx::query("DELETE FROM sprints WHERE id = $1 AND project_id = $2")
        .bind(sprint_id)
        .bind(project_id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Sprint not found".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}
