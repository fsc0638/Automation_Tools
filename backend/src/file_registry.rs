use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, FromRow, Clone)]
pub struct WorkspaceFile {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub organization_id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Option<Uuid>,
    pub source_type: String,
    pub logical_path: String,
    pub storage_path: String,
    pub classification: String,
    pub encryption_state: String,
    pub content_hash: Option<String>,
    pub size_bytes: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow, Clone)]
pub struct FileVersion {
    pub id: Uuid,
    pub file_id: Uuid,
    pub version: i32,
    pub content_hash: String,
    pub size_bytes: i64,
    pub storage_path: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow, Clone)]
pub struct FileAccessAudit {
    pub id: Uuid,
    pub file_id: Uuid,
    pub actor_user_id: Option<Uuid>,
    pub operation: String,
    pub agent_name: Option<String>,
    pub task_id: Option<Uuid>,
    pub ip_addr: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RegisterWorkspaceFile {
    pub owner_user_id: Uuid,
    pub organization_id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Option<Uuid>,
    pub source_type: String,
    pub logical_path: String,
    pub storage_path: String,
    pub classification: String,
    pub encryption_state: String,
    pub content_hash: Option<String>,
    pub size_bytes: Option<i64>,
}

pub async fn register_workspace_file(
    db: &PgPool,
    input: RegisterWorkspaceFile,
) -> AppResult<WorkspaceFile> {
    let file: WorkspaceFile = sqlx::query_as(
        "INSERT INTO workspace_files
            (owner_user_id, organization_id, workspace_id, project_id, source_type,
             logical_path, storage_path, classification, encryption_state, content_hash, size_bytes)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
         RETURNING *",
    )
    .bind(input.owner_user_id)
    .bind(input.organization_id)
    .bind(input.workspace_id)
    .bind(input.project_id)
    .bind(input.source_type)
    .bind(input.logical_path)
    .bind(input.storage_path)
    .bind(input.classification)
    .bind(input.encryption_state)
    .bind(input.content_hash)
    .bind(input.size_bytes)
    .fetch_one(db)
    .await?;

    Ok(file)
}

pub async fn audit_file_access(
    db: &PgPool,
    file_id: Uuid,
    actor_user_id: Option<Uuid>,
    operation: &str,
    agent_name: Option<&str>,
    task_id: Option<Uuid>,
    ip_addr: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO file_access_audit
            (file_id, actor_user_id, operation, agent_name, task_id, ip_addr)
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(file_id)
    .bind(actor_user_id)
    .bind(operation)
    .bind(agent_name)
    .bind(task_id)
    .bind(ip_addr)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn require_file_access(
    db: &PgPool,
    file_id: Uuid,
    user_id: Uuid,
    min_role: &str,
) -> AppResult<WorkspaceFile> {
    let file: Option<WorkspaceFile> = sqlx::query_as(
        "SELECT wf.*
           FROM workspace_files wf
          WHERE wf.id = $1
            AND (
                wf.owner_user_id = $2
                OR (wf.project_id IS NOT NULL AND user_can_access_project(wf.project_id, $2, $3))
                OR EXISTS (
                    SELECT 1 FROM workspace_members wm
                     WHERE wm.workspace_id = wf.workspace_id
                       AND wm.user_id = $2
                       AND access_role_rank(wm.role) >= access_role_rank($3)
                )
                OR EXISTS (
                    SELECT 1 FROM organization_members om
                     WHERE om.organization_id = wf.organization_id
                       AND om.user_id = $2
                       AND access_role_rank(om.role) >= access_role_rank($3)
                )
            )",
    )
    .bind(file_id)
    .bind(user_id)
    .bind(min_role)
    .fetch_optional(db)
    .await?;

    file.ok_or_else(|| AppError::NotFound("File not found".into()))
}

pub async fn latest_version_number(db: &PgPool, file_id: Uuid) -> AppResult<i32> {
    let n: Option<i32> = sqlx::query_scalar(
        "SELECT MAX(version) FROM file_versions WHERE file_id = $1",
    )
    .bind(file_id)
    .fetch_one(db)
    .await?;
    Ok(n.unwrap_or(0))
}
