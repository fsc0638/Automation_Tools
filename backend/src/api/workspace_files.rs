use axum::{
    extract::{Path, Query, State},
    response::Json,
    routing::get,
    Extension, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    error::AppResult,
    file_registry::{
        audit_file_access, require_file_access, FileAccessAudit, FileVersion, WorkspaceFile,
    },
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/workspace-files", get(list_workspace_files))
        .route("/workspace-files/:id", get(get_workspace_file))
        .route("/workspace-files/:id/versions", get(list_file_versions))
        .route("/workspace-files/:id/audit", get(list_file_audit))
}

#[derive(Debug, Deserialize, Default)]
pub struct ListWorkspaceFilesQuery {
    pub workspace_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
}

async fn list_workspace_files(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(query): Query<ListWorkspaceFilesQuery>,
) -> AppResult<Json<Vec<WorkspaceFile>>> {
    let files: Vec<WorkspaceFile> = sqlx::query_as(
        "SELECT wf.*
           FROM workspace_files wf
          WHERE ($1::uuid IS NULL OR wf.workspace_id = $1)
            AND ($2::uuid IS NULL OR wf.project_id = $2)
            AND (
                wf.owner_user_id = $3
                OR (wf.project_id IS NOT NULL AND user_can_access_project(wf.project_id, $3, 'viewer'))
                OR EXISTS (
                    SELECT 1 FROM workspace_members wm
                     WHERE wm.workspace_id = wf.workspace_id
                       AND wm.user_id = $3
                       AND access_role_rank(wm.role) >= access_role_rank('viewer')
                )
                OR EXISTS (
                    SELECT 1 FROM organization_members om
                     WHERE om.organization_id = wf.organization_id
                       AND om.user_id = $3
                       AND access_role_rank(om.role) >= access_role_rank('viewer')
                )
            )
          ORDER BY wf.updated_at DESC
          LIMIT 500",
    )
    .bind(query.workspace_id)
    .bind(query.project_id)
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(files))
}

async fn get_workspace_file(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<WorkspaceFile>> {
    let file = require_file_access(&state.db, id, auth_user.id, "viewer").await?;
    let _ = audit_file_access(&state.db, id, Some(auth_user.id), "read", None, None, None).await;
    Ok(Json(file))
}

async fn list_file_versions(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<FileVersion>>> {
    require_file_access(&state.db, id, auth_user.id, "viewer").await?;
    let rows: Vec<FileVersion> = sqlx::query_as(
        "SELECT * FROM file_versions WHERE file_id = $1 ORDER BY version DESC",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn list_file_audit(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<FileAccessAudit>>> {
    require_file_access(&state.db, id, auth_user.id, "viewer").await?;
    let rows: Vec<FileAccessAudit> = sqlx::query_as(
        "SELECT * FROM file_access_audit WHERE file_id = $1 ORDER BY created_at DESC LIMIT 200",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
