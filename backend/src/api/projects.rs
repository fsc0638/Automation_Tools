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
    db::models::{GitIdentity, Project},
    error::{AppError, AppResult},
    git_ops::manager::{
        checkout_branch, clone_repository, git_status, list_branches, list_files,
        list_remote_branches, read_file_content, GitCredentials,
    },
};

#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub description: Option<String>,
    pub source_type: String,
    pub source_path: String,
    pub git_identity_id: Option<Uuid>,
    pub default_branch: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SwitchBranchRequest {
    pub branch: String,
}

#[derive(Debug, Deserialize)]
pub struct RemoteBranchesRequest {
    pub url: String,
    pub git_identity_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub children: Option<Vec<FileNode>>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/:id", get(get_project).delete(delete_project))
        .route("/projects/:id/files", get(get_file_tree))
        .route("/projects/:id/files/content", get(get_file_content))
        .route("/projects/:id/git/status", get(get_git_status))
        .route("/projects/:id/git/branches", get(get_git_branches))
        .route("/projects/:id/git/checkout", post(switch_git_branch))
        .route("/git/remote-branches", post(get_remote_branches))
}

async fn list_projects(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> AppResult<Json<Vec<Project>>> {
    let projects: Vec<Project> = sqlx::query_as(
        "SELECT * FROM projects WHERE user_id = $1 ORDER BY updated_at DESC",
    )
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(projects))
}

async fn create_project(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateProjectRequest>,
) -> AppResult<(StatusCode, Json<Project>)> {
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("Project name is required".into()));
    }
    if req.source_type != "local" && req.source_type != "git" {
        return Err(AppError::BadRequest("source_type must be 'local' or 'git'".into()));
    }

    let identity = if req.source_type == "git" {
        match req.git_identity_id {
            Some(id) => Some(find_git_identity(&state, id, auth_user.id).await?),
            None => None,
        }
    } else {
        None
    };
    let credentials = match identity.as_ref() {
        Some(id) => Some(identity_credentials(id, &state.cipher)?),
        None => None,
    };

    let local_path = if req.source_type == "git" {
        let clone_dir = format!("./data/projects/{}/{}", auth_user.id, Uuid::new_v4());
        clone_repository(
            &req.source_path,
            &clone_dir,
            credentials.as_ref(),
            req.default_branch.as_deref(),
        )
        .map_err(|e| AppError::Git(e.to_string()))?;
        Some(clone_dir)
    } else {
        None
    };

    let project: Project = sqlx::query_as(
        "INSERT INTO projects (user_id, name, description, source_type, source_path, local_path, default_branch, git_identity_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING *",
    )
    .bind(auth_user.id)
    .bind(req.name.trim())
    .bind(&req.description)
    .bind(&req.source_type)
    .bind(&req.source_path)
    .bind(&local_path)
    .bind(req.default_branch.as_deref().unwrap_or("main"))
    .bind(req.git_identity_id)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(project)))
}

async fn get_project(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Project>> {
    let project = find_project(&state, id, auth_user.id).await?;
    Ok(Json(project))
}

async fn delete_project(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    find_project(&state, id, auth_user.id).await?;
    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_file_tree(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<FileNode>>> {
    let project = find_project(&state, id, auth_user.id).await?;
    let root = project_root_path(&project);
    let nodes = list_files(&root, &root, 0)
        .map_err(|e| AppError::Git(e.to_string()))?;
    Ok(Json(nodes))
}

async fn get_file_content(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> AppResult<Json<serde_json::Value>> {
    let project = find_project(&state, id, auth_user.id).await?;
    let root = project_root_path(&project);
    let file_path = params
        .get("path")
        .ok_or_else(|| AppError::BadRequest("path query param required".into()))?;

    let content = read_file_content(&root, file_path)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(serde_json::json!({ "path": file_path, "content": content })))
}

async fn get_git_status(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let project = find_project(&state, id, auth_user.id).await?;
    require_git_project(&project)?;
    let root = project_root_path(&project);
    let status = git_status(&root).map_err(|e| AppError::Git(e.to_string()))?;
    Ok(Json(status))
}

async fn get_git_branches(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let project = find_project(&state, id, auth_user.id).await?;
    require_git_project(&project)?;
    let root = project_root_path(&project);
    let branches = list_branches(&root).map_err(|e| AppError::Git(e.to_string()))?;
    Ok(Json(serde_json::json!({ "branches": branches })))
}

async fn switch_git_branch(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<SwitchBranchRequest>,
) -> AppResult<Json<Project>> {
    let project = find_project(&state, id, auth_user.id).await?;
    require_git_project(&project)?;
    let root = project_root_path(&project);

    let identity = match project.git_identity_id {
        Some(identity_id) => Some(find_git_identity(&state, identity_id, auth_user.id).await?),
        None => None,
    };
    let credentials = match identity.as_ref() {
        Some(id) => Some(identity_credentials(id, &state.cipher)?),
        None => None,
    };

    checkout_branch(&root, &req.branch, credentials.as_ref())
        .map_err(|e| AppError::Git(e.to_string()))?;

    let updated: Project = sqlx::query_as(
        "UPDATE projects SET default_branch = $1, updated_at = NOW() WHERE id = $2 AND user_id = $3 RETURNING *",
    )
    .bind(req.branch.trim())
    .bind(id)
    .bind(auth_user.id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(updated))
}

async fn get_remote_branches(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<RemoteBranchesRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let identity = match req.git_identity_id {
        Some(id) => Some(find_git_identity(&state, id, auth_user.id).await?),
        None => None,
    };
    let credentials = match identity.as_ref() {
        Some(id) => Some(identity_credentials(id, &state.cipher)?),
        None => None,
    };

    let branches = list_remote_branches(&req.url, credentials.as_ref())
        .await
        .map_err(|e| AppError::Git(e.to_string()))?;

    Ok(Json(serde_json::json!({ "branches": branches })))
}

async fn find_project(state: &AppState, id: Uuid, user_id: Uuid) -> AppResult<Project> {
    let project: Option<Project> =
        sqlx::query_as("SELECT * FROM projects WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;

    project.ok_or_else(|| AppError::NotFound("Project not found".into()))
}

async fn find_git_identity(state: &AppState, id: Uuid, user_id: Uuid) -> AppResult<GitIdentity> {
    let identity: Option<GitIdentity> = sqlx::query_as(
        "SELECT * FROM git_identities WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    identity.ok_or_else(|| AppError::NotFound("Git identity not found".into()))
}

fn identity_credentials(
    identity: &GitIdentity,
    cipher: &crate::crypto::TokenCipher,
) -> AppResult<GitCredentials> {
    let access_token = cipher
        .decrypt(&identity.access_token)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("token decrypt failed: {}", e)))?;
    Ok(GitCredentials {
        username: identity.username.clone(),
        access_token,
    })
}

fn require_git_project(project: &Project) -> AppResult<()> {
    if project.source_type != "git" {
        return Err(AppError::BadRequest("Project is not a git repository".into()));
    }
    Ok(())
}

fn project_root_path(project: &Project) -> String {
    if project.source_type == "git" {
        project.local_path.clone().unwrap_or_else(|| project.source_path.clone())
    } else {
        project.source_path.clone()
    }
}
