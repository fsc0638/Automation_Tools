use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    db::models::Project,
    error::{AppError, AppResult},
    git_ops::manager::{clone_repository, list_files, read_file_content},
};

#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub description: Option<String>,
    pub source_type: String,
    pub source_path: String,
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
    if req.name.is_empty() {
        return Err(AppError::BadRequest("Project name is required".into()));
    }
    if req.source_type != "local" && req.source_type != "git" {
        return Err(AppError::BadRequest("source_type must be 'local' or 'git'".into()));
    }

    let local_path = if req.source_type == "git" {
        let clone_dir = format!(
            "./data/projects/{}/{}",
            auth_user.id,
            Uuid::new_v4()
        );
        clone_repository(&req.source_path, &clone_dir)
            .map_err(|e| AppError::Git(e.to_string()))?;
        Some(clone_dir)
    } else {
        None
    };

    let project: Project = sqlx::query_as(
        "INSERT INTO projects (user_id, name, description, source_type, source_path, local_path)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING *",
    )
    .bind(auth_user.id)
    .bind(&req.name)
    .bind(&req.description)
    .bind(&req.source_type)
    .bind(&req.source_path)
    .bind(&local_path)
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
    use crate::git_ops::manager::git_status;
    let project = find_project(&state, id, auth_user.id).await?;
    let root = project_root_path(&project);
    let status = git_status(&root).map_err(|e| AppError::Git(e.to_string()))?;
    Ok(Json(status))
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

fn project_root_path(project: &Project) -> String {
    if project.source_type == "git" {
        project.local_path.clone().unwrap_or_else(|| project.source_path.clone())
    } else {
        project.source_path.clone()
    }
}
