use axum::{
    extract::{Path, Query, State},
    response::Json,
    routing::{get, patch, post},
    Extension, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    error::{AppError, AppResult},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent-tasks", get(list_agent_tasks).post(create_agent_task))
        .route("/agent-tasks/:id", get(get_agent_task))
        .route("/agent-tasks/:id/lease", post(lease_agent_task))
        .route("/agent-tasks/:id/heartbeat", post(heartbeat_agent_task))
        .route("/agent-tasks/:id/complete", post(complete_agent_task))
        .route("/agent-tasks/:id/cancel", patch(cancel_agent_task))
}

#[derive(Debug, Serialize, FromRow, Clone)]
pub struct AgentTask {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub organization_id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Option<Uuid>,
    pub requested_by: Option<Uuid>,
    pub task_type: String,
    pub agent_name: String,
    pub priority: i32,
    pub status: String,
    pub input: Value,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAgentTaskRequest {
    pub project_id: Uuid,
    pub task_type: String,
    pub agent_name: String,
    pub priority: Option<i32>,
    pub input: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct LeaseAgentTaskRequest {
    pub lease_owner: String,
    pub lease_seconds: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteAgentTaskRequest {
    pub status: String,
    pub output: Option<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ListAgentTasksQuery {
    pub project_id: Option<Uuid>,
    pub status: Option<String>,
    pub limit: Option<i64>,
}

async fn list_agent_tasks(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(query): Query<ListAgentTasksQuery>,
) -> AppResult<Json<Vec<AgentTask>>> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let rows = sqlx::query_as::<_, AgentTask>(
        "SELECT at.*
           FROM agent_tasks at
          WHERE ($1::uuid IS NULL OR at.project_id = $1)
            AND ($2::text IS NULL OR at.status = $2)
            AND (
                at.owner_user_id = $3
                OR (at.project_id IS NOT NULL AND user_can_access_project(at.project_id, $3, 'viewer'))
                OR EXISTS (
                    SELECT 1 FROM workspace_members wm
                     WHERE wm.workspace_id = at.workspace_id
                       AND wm.user_id = $3
                       AND access_role_rank(wm.role) >= access_role_rank('viewer')
                )
                OR EXISTS (
                    SELECT 1 FROM organization_members om
                     WHERE om.organization_id = at.organization_id
                       AND om.user_id = $3
                       AND access_role_rank(om.role) >= access_role_rank('viewer')
                )
            )
          ORDER BY at.priority ASC, at.created_at DESC
          LIMIT $4",
    )
    .bind(query.project_id)
    .bind(query.status)
    .bind(auth_user.id)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn create_agent_task(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateAgentTaskRequest>,
) -> AppResult<Json<AgentTask>> {
    let project = require_project_access(&state, req.project_id, auth_user.id, "editor").await?;
    let input = req.input.unwrap_or_else(|| serde_json::json!({}));

    let row = sqlx::query_as::<_, AgentTask>(
        "INSERT INTO agent_tasks
            (owner_user_id, organization_id, workspace_id, project_id, requested_by,
             task_type, agent_name, priority, input)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
         RETURNING *",
    )
    .bind(project.user_id)
    .bind(project.organization_id)
    .bind(project.workspace_id)
    .bind(project.id)
    .bind(auth_user.id)
    .bind(req.task_type)
    .bind(req.agent_name)
    .bind(req.priority.unwrap_or(100))
    .bind(input)
    .fetch_one(&state.db)
    .await?;

    insert_agent_task_event(&state, row.id, Some(auth_user.id), Some(&row.agent_name), "created", None, None).await?;
    Ok(Json(row))
}

async fn get_agent_task(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<AgentTask>> {
    let row = require_agent_task_access(&state, id, auth_user.id, "viewer").await?;
    Ok(Json(row))
}

async fn lease_agent_task(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<LeaseAgentTaskRequest>,
) -> AppResult<Json<AgentTask>> {
    if req.lease_owner.trim().is_empty() {
        return Err(AppError::BadRequest("lease_owner is required".into()));
    }
    require_agent_task_access(&state, id, auth_user.id, "editor").await?;
    let lease_seconds = req.lease_seconds.unwrap_or(900).clamp(60, 3600);

    let row = sqlx::query_as::<_, AgentTask>(
        "UPDATE agent_tasks
            SET status = 'leased',
                lease_owner = $2,
                lease_expires_at = NOW() + ($3::text || ' seconds')::interval,
                started_at = COALESCE(started_at, NOW()),
                updated_at = NOW()
          WHERE id = $1
            AND (status = 'queued' OR lease_expires_at < NOW())
          RETURNING *",
    )
    .bind(id)
    .bind(req.lease_owner)
    .bind(lease_seconds)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Conflict("Task is not available for lease".into()))?;

    insert_agent_task_event(&state, id, Some(auth_user.id), row.lease_owner.as_deref(), "leased", None, None).await?;
    Ok(Json(row))
}

async fn heartbeat_agent_task(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<LeaseAgentTaskRequest>,
) -> AppResult<Json<AgentTask>> {
    require_agent_task_access(&state, id, auth_user.id, "editor").await?;
    let lease_seconds = req.lease_seconds.unwrap_or(900).clamp(60, 3600);
    let row = sqlx::query_as::<_, AgentTask>(
        "UPDATE agent_tasks
            SET status = 'running',
                lease_owner = $2,
                lease_expires_at = NOW() + ($3::text || ' seconds')::interval,
                updated_at = NOW()
          WHERE id = $1 AND status IN ('leased','running')
          RETURNING *",
    )
    .bind(id)
    .bind(req.lease_owner)
    .bind(lease_seconds)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Conflict("Task is not leased/running".into()))?;

    insert_agent_task_event(&state, id, Some(auth_user.id), row.lease_owner.as_deref(), "heartbeat", None, None).await?;
    Ok(Json(row))
}

async fn complete_agent_task(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<CompleteAgentTaskRequest>,
) -> AppResult<Json<AgentTask>> {
    require_agent_task_access(&state, id, auth_user.id, "editor").await?;
    if !matches!(req.status.as_str(), "completed" | "failed") {
        return Err(AppError::BadRequest("status must be completed or failed".into()));
    }

    let row = sqlx::query_as::<_, AgentTask>(
        "UPDATE agent_tasks
            SET status = $2,
                output = $3,
                error = $4,
                completed_at = NOW(),
                lease_expires_at = NULL,
                updated_at = NOW()
          WHERE id = $1 AND status IN ('leased','running')
          RETURNING *",
    )
    .bind(id)
    .bind(req.status.clone())
    .bind(req.output)
    .bind(req.error.clone())
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Conflict("Task is not leased/running".into()))?;

    insert_agent_task_event(&state, id, Some(auth_user.id), row.lease_owner.as_deref(), &req.status, req.error.as_deref(), row.output.clone()).await?;
    Ok(Json(row))
}

async fn cancel_agent_task(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<AgentTask>> {
    require_agent_task_access(&state, id, auth_user.id, "editor").await?;
    let row = sqlx::query_as::<_, AgentTask>(
        "UPDATE agent_tasks
            SET status = 'cancelled', completed_at = NOW(), lease_expires_at = NULL, updated_at = NOW()
          WHERE id = $1 AND status NOT IN ('completed','failed','cancelled')
          RETURNING *",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Conflict("Task cannot be cancelled".into()))?;

    insert_agent_task_event(&state, id, Some(auth_user.id), row.lease_owner.as_deref(), "cancelled", None, None).await?;
    Ok(Json(row))
}

async fn require_project_access(
    state: &AppState,
    project_id: Uuid,
    user_id: Uuid,
    min_role: &str,
) -> AppResult<crate::db::models::Project> {
    sqlx::query_as::<_, crate::db::models::Project>(
        "SELECT *, user_project_role(id, $2) AS effective_role
           FROM projects
          WHERE id = $1 AND user_can_access_project(id, $2, $3)",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(min_role)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Project not found".into()))
}

async fn require_agent_task_access(
    state: &AppState,
    task_id: Uuid,
    user_id: Uuid,
    min_role: &str,
) -> AppResult<AgentTask> {
    sqlx::query_as::<_, AgentTask>(
        "SELECT at.*
           FROM agent_tasks at
          WHERE at.id = $1
            AND (
                at.owner_user_id = $2
                OR (at.project_id IS NOT NULL AND user_can_access_project(at.project_id, $2, $3))
                OR EXISTS (
                    SELECT 1 FROM workspace_members wm
                     WHERE wm.workspace_id = at.workspace_id
                       AND wm.user_id = $2
                       AND access_role_rank(wm.role) >= access_role_rank($3)
                )
                OR EXISTS (
                    SELECT 1 FROM organization_members om
                     WHERE om.organization_id = at.organization_id
                       AND om.user_id = $2
                       AND access_role_rank(om.role) >= access_role_rank($3)
                )
            )",
    )
    .bind(task_id)
    .bind(user_id)
    .bind(min_role)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Agent task not found".into()))
}

async fn insert_agent_task_event(
    state: &AppState,
    task_id: Uuid,
    actor_user_id: Option<Uuid>,
    agent_name: Option<&str>,
    event_type: &str,
    note: Option<&str>,
    payload: Option<Value>,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO agent_task_events
            (task_id, actor_user_id, agent_name, event_type, note, payload)
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(task_id)
    .bind(actor_user_id)
    .bind(agent_name)
    .bind(event_type)
    .bind(note)
    .bind(payload.unwrap_or_else(|| serde_json::json!({})))
    .execute(&state.db)
    .await?;
    Ok(())
}
