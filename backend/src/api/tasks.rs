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
pub struct ProjectTask {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub why: Option<String>,
    pub affected_files: Option<Vec<String>>,
    pub acceptance_criteria: Option<String>,
    pub estimated_effort: Option<String>,
    pub priority: String,
    pub status: String,
    pub source_message_id: Option<Uuid>,
    /// Conversation that contains source_message_id. Resolved via LEFT JOIN
    /// so the frontend can deep-link from a Roadmap card back to the chat.
    pub source_conversation_id: Option<Uuid>,
    // P1 fields
    pub assignee: Option<String>,
    pub due_date: Option<NaiveDate>,
    pub test_plan: Option<String>,
    pub rollback_plan: Option<String>,
    pub definition_of_done: Option<String>,
    /// Always present (NOT NULL DEFAULT '{}') so the frontend can render
    /// chips without a null-check.
    pub labels: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const TASK_SELECT: &str = "SELECT t.id, t.project_id, t.title, t.why, t.affected_files,
        t.acceptance_criteria, t.estimated_effort, t.priority, t.status,
        t.source_message_id, m.conversation_id AS source_conversation_id,
        t.assignee, t.due_date, t.test_plan, t.rollback_plan,
        t.definition_of_done, t.labels,
        t.created_at, t.updated_at
     FROM project_tasks t
     LEFT JOIN messages m ON m.id = t.source_message_id";

#[derive(Debug, Deserialize)]
pub struct CreateTask {
    pub title: String,
    pub why: Option<String>,
    pub affected_files: Option<Vec<String>>,
    pub acceptance_criteria: Option<String>,
    pub estimated_effort: Option<String>,
    pub priority: Option<String>,
    pub source_message_id: Option<Uuid>,
    pub assignee: Option<String>,
    pub due_date: Option<NaiveDate>,
    pub test_plan: Option<String>,
    pub rollback_plan: Option<String>,
    pub definition_of_done: Option<String>,
    pub labels: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTask {
    pub title: Option<String>,
    pub why: Option<String>,
    pub affected_files: Option<Vec<String>>,
    pub acceptance_criteria: Option<String>,
    pub estimated_effort: Option<String>,
    pub priority: Option<String>,
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub due_date: Option<NaiveDate>,
    pub test_plan: Option<String>,
    pub rollback_plan: Option<String>,
    pub definition_of_done: Option<String>,
    pub labels: Option<Vec<String>>,
    /// Optional explanation attached to a status transition; recorded
    /// in task_status_history. Ignored when status doesn't actually change.
    pub status_note: Option<String>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct TaskStatusEvent {
    pub id: Uuid,
    pub task_id: Uuid,
    pub from_status: Option<String>,
    pub to_status: String,
    pub changed_by: Option<Uuid>,
    pub changed_by_name: Option<String>,
    pub note: Option<String>,
    pub changed_at: DateTime<Utc>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/projects/:id/tasks", get(list_tasks).post(create_task))
        .route(
            "/projects/:id/tasks/:task_id",
            http_patch(update_task).delete(delete_task),
        )
        .route(
            "/projects/:id/tasks/:task_id/history",
            get(list_task_history),
        )
}

async fn verify_access(state: &AppState, project_id: Uuid, user_id: Uuid) -> AppResult<()> {
    let exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM projects WHERE id = $1 AND user_id = $2")
            .bind(project_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("Project not found".into()));
    }
    Ok(())
}

async fn list_tasks(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
) -> AppResult<Json<Vec<ProjectTask>>> {
    verify_access(&state, project_id, auth_user.id).await?;
    let sql = format!(
        "{TASK_SELECT}
         WHERE t.project_id = $1
         ORDER BY
            CASE t.priority WHEN 'critical' THEN 0 WHEN 'high' THEN 1 WHEN 'medium' THEN 2 ELSE 3 END,
            t.created_at DESC"
    );
    let tasks: Vec<ProjectTask> = sqlx::query_as(&sql)
        .bind(project_id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(tasks))
}

async fn create_task(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
    Json(req): Json<CreateTask>,
) -> AppResult<(StatusCode, Json<ProjectTask>)> {
    verify_access(&state, project_id, auth_user.id).await?;
    if req.title.trim().is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }

    let priority = match req.priority.as_deref() {
        Some(p @ ("low" | "medium" | "high" | "critical")) => p.to_string(),
        _ => "medium".to_string(),
    };

    let labels = req.labels.unwrap_or_default();

    let new_id: (Uuid,) = sqlx::query_as(
        "INSERT INTO project_tasks
         (project_id, title, why, affected_files, acceptance_criteria,
          estimated_effort, priority, source_message_id,
          assignee, due_date, test_plan, rollback_plan, definition_of_done, labels)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
         RETURNING id",
    )
    .bind(project_id)
    .bind(req.title.trim())
    .bind(req.why.as_deref())
    .bind(req.affected_files.as_deref())
    .bind(req.acceptance_criteria.as_deref())
    .bind(req.estimated_effort.as_deref())
    .bind(&priority)
    .bind(req.source_message_id)
    .bind(req.assignee.as_deref())
    .bind(req.due_date)
    .bind(req.test_plan.as_deref())
    .bind(req.rollback_plan.as_deref())
    .bind(req.definition_of_done.as_deref())
    .bind(&labels)
    .fetch_one(&state.db)
    .await?;

    // Initial transition: NULL → "todo" (or whatever was set).
    let _ = sqlx::query(
        "INSERT INTO task_status_history (task_id, from_status, to_status, changed_by, note)
         VALUES ($1, NULL, 'todo', $2, $3)",
    )
    .bind(new_id.0)
    .bind(auth_user.id)
    .bind(Some("created"))
    .execute(&state.db)
    .await;

    let sql = format!("{TASK_SELECT} WHERE t.id = $1");
    let task: ProjectTask = sqlx::query_as(&sql)
        .bind(new_id.0)
        .fetch_one(&state.db)
        .await?;

    Ok((StatusCode::CREATED, Json(task)))
}

async fn update_task(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateTask>,
) -> AppResult<Json<ProjectTask>> {
    verify_access(&state, project_id, auth_user.id).await?;

    if let Some(s) = req.status.as_deref() {
        if !matches!(s, "todo" | "in-progress" | "done" | "cancelled") {
            return Err(AppError::BadRequest("invalid status".into()));
        }
    }
    if let Some(p) = req.priority.as_deref() {
        if !matches!(p, "low" | "medium" | "high" | "critical") {
            return Err(AppError::BadRequest("invalid priority".into()));
        }
    }

    // Capture the pre-update status so we know if a transition happened.
    let prev: Option<(String,)> = sqlx::query_as(
        "SELECT status FROM project_tasks WHERE id = $1 AND project_id = $2",
    )
    .bind(task_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;
    let prev_status = prev.map(|(s,)| s);

    let updated: Option<(Uuid,)> = sqlx::query_as(
        "UPDATE project_tasks SET
            title = COALESCE($1, title),
            why = COALESCE($2, why),
            affected_files = COALESCE($3, affected_files),
            acceptance_criteria = COALESCE($4, acceptance_criteria),
            estimated_effort = COALESCE($5, estimated_effort),
            priority = COALESCE($6, priority),
            status = COALESCE($7, status),
            assignee = COALESCE($8, assignee),
            due_date = COALESCE($9, due_date),
            test_plan = COALESCE($10, test_plan),
            rollback_plan = COALESCE($11, rollback_plan),
            definition_of_done = COALESCE($12, definition_of_done),
            labels = COALESCE($13, labels),
            updated_at = NOW()
         WHERE id = $14 AND project_id = $15
         RETURNING id",
    )
    .bind(req.title.as_deref().map(str::trim))
    .bind(req.why.as_deref())
    .bind(req.affected_files.as_deref())
    .bind(req.acceptance_criteria.as_deref())
    .bind(req.estimated_effort.as_deref())
    .bind(req.priority.as_deref())
    .bind(req.status.as_deref())
    .bind(req.assignee.as_deref())
    .bind(req.due_date)
    .bind(req.test_plan.as_deref())
    .bind(req.rollback_plan.as_deref())
    .bind(req.definition_of_done.as_deref())
    .bind(req.labels.as_deref())
    .bind(task_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;

    let id = updated.ok_or_else(|| AppError::NotFound("Task not found".into()))?.0;

    // Record transition only when status actually changed.
    if let (Some(prev), Some(next)) = (prev_status.as_deref(), req.status.as_deref()) {
        if prev != next {
            let _ = sqlx::query(
                "INSERT INTO task_status_history (task_id, from_status, to_status, changed_by, note)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(id)
            .bind(prev)
            .bind(next)
            .bind(auth_user.id)
            .bind(req.status_note.as_deref())
            .execute(&state.db)
            .await;
        }
    }

    let sql = format!("{TASK_SELECT} WHERE t.id = $1");
    let task: ProjectTask = sqlx::query_as(&sql)
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(task))
}

async fn delete_task(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    verify_access(&state, project_id, auth_user.id).await?;
    let result = sqlx::query("DELETE FROM project_tasks WHERE id = $1 AND project_id = $2")
        .bind(task_id)
        .bind(project_id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Task not found".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn list_task_history(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Vec<TaskStatusEvent>>> {
    verify_access(&state, project_id, auth_user.id).await?;

    // Confirm task belongs to this project (avoid leaking other projects' history).
    let exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM project_tasks WHERE id = $1 AND project_id = $2",
    )
    .bind(task_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("Task not found".into()));
    }

    let events: Vec<TaskStatusEvent> = sqlx::query_as(
        "SELECT h.id, h.task_id, h.from_status, h.to_status,
                h.changed_by, u.display_name AS changed_by_name,
                h.note, h.changed_at
         FROM task_status_history h
         LEFT JOIN users u ON u.id = h.changed_by
         WHERE h.task_id = $1
         ORDER BY h.changed_at ASC",
    )
    .bind(task_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(events))
}
