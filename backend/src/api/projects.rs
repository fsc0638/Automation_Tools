use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use std::{fs, io::Cursor, path::Path as FsPath};
use uuid::Uuid;
use zip::ZipArchive;

use crate::{
    api::{auth::AuthUser, project_index::rebuild_project_index, AppState},
    db::models::{GitIdentity, Project},
    error::{AppError, AppResult},
    git_ops::manager::{
        checkout_branch, clone_repository, git_status, list_branches, list_files,
        list_remote_branches, read_file_content, sync_current_branch, GitCredentials, SyncResult,
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
        .route(
            "/projects/upload",
            post(upload_project).layer(DefaultBodyLimit::max(100 * 1024 * 1024)),
        )
        .route("/projects/:id", get(get_project).delete(delete_project))
        .route("/projects/:id/files", get(get_file_tree))
        .route("/projects/:id/files/content", get(get_file_content))
        .route("/projects/:id/index", post(reindex_project))
        .route("/projects/:id/git/status", get(get_git_status))
        .route("/projects/:id/git/branches", get(get_git_branches))
        .route("/projects/:id/git/checkout", post(switch_git_branch))
        .route("/projects/:id/git/sync", post(sync_git_repo))
        .route("/git/remote-branches", post(get_remote_branches))
}

async fn list_projects(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> AppResult<Json<Vec<Project>>> {
    let projects: Vec<Project> =
        sqlx::query_as("SELECT * FROM projects WHERE user_id = $1 ORDER BY updated_at DESC")
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
    if req.source_type != "local" && req.source_type != "git" && req.source_type != "upload" {
        return Err(AppError::BadRequest(
            "source_type must be 'local', 'git', or 'upload'".into(),
        ));
    }
    if req.source_type == "upload" {
        return Err(AppError::BadRequest(
            "Use /projects/upload with a zip file for upload projects".into(),
        ));
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
        let clone_dir = build_clone_dir(
            &state.config.project_data_root,
            &req.source_path,
            Uuid::new_v4(),
        );
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

    let root = project_root_path(&project);
    let _ = rebuild_project_index(&state.db, project.id, &root).await;

    Ok((StatusCode::CREATED, Json(project)))
}

async fn upload_project(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    mut multipart: Multipart,
) -> AppResult<(StatusCode, Json<Project>)> {
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut zip_bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("Invalid multipart upload: {}", e)))?
    {
        let field_name = field.name().unwrap_or_default().to_string();
        match field_name.as_str() {
            "name" => {
                name = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| AppError::BadRequest(e.to_string()))?,
                );
            }
            "description" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                if !text.trim().is_empty() {
                    description = Some(text);
                }
            }
            "file" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                zip_bytes = Some(bytes.to_vec());
            }
            _ => {}
        }
    }

    let name = name.unwrap_or_default().trim().to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("Project name is required".into()));
    }
    let zip_bytes = zip_bytes.ok_or_else(|| AppError::BadRequest("zip file is required".into()))?;
    if zip_bytes.is_empty() {
        return Err(AppError::BadRequest("zip file is empty".into()));
    }

    let upload_id = Uuid::new_v4();
    let upload_dir = build_upload_dir(&state.config.project_data_root, &name, upload_id);
    fs::create_dir_all(&upload_dir).map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    extract_zip_project(&zip_bytes, &upload_dir)?;

    let project: Project = sqlx::query_as(
        "INSERT INTO projects (user_id, name, description, source_type, source_path, local_path, default_branch, git_identity_id)
         VALUES ($1, $2, $3, 'upload', $4, $4, NULL, NULL)
         RETURNING *",
    )
    .bind(auth_user.id)
    .bind(&name)
    .bind(&description)
    .bind(&upload_dir)
    .fetch_one(&state.db)
    .await?;

    let _ = rebuild_project_index(&state.db, project.id, &upload_dir).await;

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
    let nodes = list_files(&root, &root, 0).map_err(|e| AppError::Git(e.to_string()))?;
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

    let content =
        read_file_content(&root, file_path).map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(
        serde_json::json!({ "path": file_path, "content": content }),
    ))
}

async fn reindex_project(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let project = find_project(&state, id, auth_user.id).await?;
    let root = project_root_path(&project);
    let indexed = rebuild_project_index(&state.db, project.id, &root)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    Ok(Json(serde_json::json!({ "indexed_files": indexed })))
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

    let _ = rebuild_project_index(&state.db, updated.id, &root).await;

    Ok(Json(updated))
}

async fn sync_git_repo(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
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

    let result = sync_current_branch(&root, credentials.as_ref())
        .map_err(|e| AppError::Git(e.to_string()))?;

    let _ = rebuild_project_index(&state.db, project.id, &root).await;

    let status = match result {
        SyncResult::AlreadyUpToDate => "up-to-date",
        SyncResult::FastForwarded => "fast-forwarded",
        SyncResult::NoRemoteBranch => "no-remote-branch",
    };

    Ok(Json(serde_json::json!({ "status": status })))
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
    let identity: Option<GitIdentity> =
        sqlx::query_as("SELECT * FROM git_identities WHERE id = $1 AND user_id = $2")
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
        return Err(AppError::BadRequest(
            "Project is not a git repository".into(),
        ));
    }
    Ok(())
}

fn project_root_path(project: &Project) -> String {
    if project.source_type == "git" || project.source_type == "upload" {
        project
            .local_path
            .clone()
            .unwrap_or_else(|| project.source_path.clone())
    } else {
        project.source_path.clone()
    }
}

/// Build the on-disk clone directory: `<root>/<repo-slug>-<short>`.
/// Slug is derived from the URL's last segment, sanitised; short is 8 hex
/// chars from the supplied UUID. Existing projects keep whatever path was
/// stored at create time, so old clones remain accessible after upgrades.
fn build_clone_dir(root: &str, source_url: &str, clone_id: Uuid) -> String {
    let slug = repo_slug_from_url(source_url);
    let mut short = clone_id.to_string();
    short.retain(|c| c != '-');
    let short = short.chars().take(8).collect::<String>();
    let root = root.trim_end_matches(['/', '\\']);
    format!("{}/{}-{}", root, slug, short)
}

fn build_upload_dir(root: &str, name: &str, upload_id: Uuid) -> String {
    let mut short = upload_id.to_string();
    short.retain(|c| c != '-');
    let short = short.chars().take(8).collect::<String>();
    let slug = safe_slug(name);
    let root = root.trim_end_matches(['/', '\\']);
    format!("{}/upload-{}-{}", root, slug, short)
}

fn safe_slug(input: &str) -> String {
    let safe: String = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = safe.trim_matches(['_', '.', '-']).to_string();
    if trimmed.is_empty() {
        "project".into()
    } else {
        trimmed
    }
}

fn extract_zip_project(bytes: &[u8], dest: &str) -> AppResult<()> {
    let dest_path = FsPath::new(dest);
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| AppError::BadRequest(format!("Invalid zip file: {}", e)))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| AppError::BadRequest(format!("Invalid zip entry: {}", e)))?;
        let Some(enclosed) = file.enclosed_name().map(|p| p.to_owned()) else {
            continue;
        };
        let out_path = dest_path.join(enclosed);
        if file.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
        }
        let mut out =
            fs::File::create(&out_path).map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
        std::io::copy(&mut file, &mut out).map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    }

    Ok(())
}

fn repo_slug_from_url(source_url: &str) -> String {
    let cleaned = source_url
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git");
    // Take everything after the last '/' or ':' (covers HTTPS and SSH URLs).
    let last = cleaned
        .rsplit(['/', ':'])
        .find(|s| !s.is_empty())
        .unwrap_or("repo");
    let safe: String = last
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = safe.trim_matches(['_', '.', '-']).to_string();
    if trimmed.is_empty() {
        "repo".into()
    } else {
        trimmed
    }
}
