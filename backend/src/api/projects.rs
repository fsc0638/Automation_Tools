use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post},
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use std::{fs, io::Cursor, path::Path as FsPath, sync::Arc};
use uuid::Uuid;
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

use crate::{
    api::{auth::AuthUser, project_index::rebuild_project_index_all, AppState},
    db::models::{GitIdentity, Project, ProjectSource},
    error::{AppError, AppResult},
    file_registry::{audit_file_access, register_workspace_file, RegisterWorkspaceFile},
    git_ops::manager::{
        checkout_branch, clone_repository, git_status, list_branches, list_files,
        list_remote_branches, read_file_content, sync_current_branch, GitCredentials, SyncResult,
    },
    security::vault_service::VaultService,
};

#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub description: Option<String>,
    pub source_type: String,
    pub source_path: String,
    pub git_identity_id: Option<Uuid>,
    pub default_branch: Option<String>,
    /// Workspace kind (Phase 2). Omitted ⇒ "code" (back-compat: old
    /// clients keep creating repo-backed projects exactly as before).
    /// "admin"/"general" = 行政庶務 work area: no repo, no clone, no
    /// index — just todos/meetings/notes.
    pub kind: Option<String>,
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
        .route("/projects/:id/archive", post(set_archived))
        .route("/projects/:id/files", get(get_file_tree))
        .route("/projects/:id/files/content", get(get_file_content))
        .route("/projects/:id/index", post(reindex_project))
        .route("/projects/:id/git/status", get(get_git_status))
        .route("/projects/:id/git/branches", get(get_git_branches))
        .route("/projects/:id/git/checkout", post(switch_git_branch))
        .route("/projects/:id/git/sync", post(sync_git_repo))
        .route("/projects/:id/git/remote-file", get(get_remote_file))
        .route("/projects/:id/agent/react", post(react_agent))
        .route("/projects/:id/sources", get(list_sources).post(add_source))
        .route("/projects/:id/sources/:source_id", delete(delete_source))
        .route("/git/remote-branches", post(get_remote_branches))
}

async fn list_projects(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> AppResult<Json<Vec<Project>>> {
    let projects: Vec<Project> = sqlx::query_as(
        "SELECT *, user_project_role(id, $1) AS effective_role FROM projects
         WHERE user_can_access_project(id, $1, 'viewer')
         ORDER BY updated_at DESC",
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

    // Phase 2: workspace kind. Default "code" keeps every existing
    // client/path byte-identical. admin/general are non-repo work
    // areas (行政庶務) — backend treats this as binary on is_code.
    let kind = req
        .kind
        .as_deref()
        .map(|k| k.trim().to_ascii_lowercase())
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| "code".into());
    if !matches!(kind.as_str(), "code" | "admin" | "general") {
        return Err(AppError::BadRequest(
            "kind must be 'code', 'admin', or 'general'".into(),
        ));
    }
    let is_code = kind == "code";

    if is_code {
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
    }
    // Non-code workspaces have no repo: force a neutral source_type and
    // ignore git fields entirely (clone/index are skipped below).
    let effective_source_type: &str = if is_code { &req.source_type } else { "local" };

    let identity = if is_code && req.source_type == "git" {
        match req.git_identity_id {
            Some(id) => Some(find_git_identity(&state, id, auth_user.id).await?),
            None => None,
        }
    } else {
        None
    };
    let credentials = match identity.as_ref() {
        Some(id) => Some(identity_credentials(id, &state, auth_user.id)?),
        None => None,
    };
    let (organization_id, workspace_id) = ensure_personal_workspace(&state, auth_user.id).await?;

    let local_path = if is_code && req.source_type == "git" {
        let clone_dir = build_clone_dir(
            &state.config.project_data_root,
            auth_user.id,
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
        "INSERT INTO projects (user_id, organization_id, workspace_id, name, description, source_type, source_path, local_path, default_branch, git_identity_id, kind)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         RETURNING *",
    )
    .bind(auth_user.id)
    .bind(organization_id)
    .bind(workspace_id)
    .bind(req.name.trim())
    .bind(&req.description)
    .bind(effective_source_type)
    .bind(&req.source_path)
    .bind(&local_path)
    .bind(req.default_branch.as_deref().unwrap_or("main"))
    .bind(if is_code { req.git_identity_id } else { None })
    .bind(&kind)
    .fetch_one(&state.db)
    .await?;
    grant_project_owner(&state, project.id, auth_user.id).await?;

    // ── Vault-seal the git clone as an encrypted backup ──────────────────
    // Best-effort: a large or failed zip must never block project creation.
    // Repos > 100 MB uncompressed are skipped — they rely on OS FileVault
    // (macOS FileVault / BitLocker) as the disk-level protection layer.
    if let Some(clone_dir) = &local_path {
        match state.session_keys.get_cipher(auth_user.id) {
            Some(user_kek) => {
                let dir_path = std::path::Path::new(clone_dir);
                match zip_dir_bytes(dir_path, 100 * 1024 * 1024) {
                    Ok(zip_bytes) => {
                        let vsvc = VaultService::for_user(
                            &state.db,
                            Arc::new(user_kek),
                            auth_user.id,
                            state.cipher.clone(),
                            auth_user.id,
                            None,
                        );
                        if let Err(e) = vsvc.seal("vault_file", project.id, &zip_bytes).await {
                            tracing::warn!(
                                project_id = %project.id,
                                "create_project: vault seal failed (best-effort): {e}"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            project_id = %project.id,
                            "create_project: git repo vault seal skipped: {e}"
                        );
                    }
                }
            }
            None => {
                tracing::warn!(
                    project_id = %project.id,
                    "create_project: no User KEK in session — git vault seal skipped"
                );
            }
        }
    }

    if let Some(clone_dir) = &local_path {
        register_project_storage_file(
            &state,
            &project,
            auth_user.id,
            "git",
            repo_slug_from_url(&req.source_path),
            clone_dir.clone(),
            dir_bytes_recursive(std::path::Path::new(clone_dir)).ok().map(|n| n as i64),
            "git_clone",
        )
        .await;
    }

    // MS-2: register this initial source as the workspace's first
    // project_sources row (canonical multi-source list). Only when it
    // actually has a path — a name-only admin workspace has no source.
    if !project.source_path.trim().is_empty() {
        let _ = sqlx::query(
            "INSERT INTO project_sources
                (project_id, kind, source_path, local_path, git_identity_id, default_branch, label)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(project.id)
        .bind(&project.source_type)
        .bind(&project.source_path)
        .bind(&project.local_path)
        .bind(project.git_identity_id)
        .bind(project.default_branch.as_deref())
        .bind(project.name.trim())
        .execute(&state.db)
        .await;
    }

    // Index when there are files to index: code workspaces always, OR
    // an admin/general workspace that was given a local folder path
    // (the user wants AI answers grounded on that folder's content).
    if is_code || !project.source_path.trim().is_empty() {
        // Background — a large repo/folder embeds many chunks; don't
        // make workspace creation hang on it.
        let db = state.db.clone();
        let proj = project.clone();
        tokio::spawn(async move {
            let _ = rebuild_project_index_all(&db, &proj).await;
        });
    }

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
    let (organization_id, workspace_id) = ensure_personal_workspace(&state, auth_user.id).await?;

    let upload_id = Uuid::new_v4();
    let upload_dir = build_upload_dir(&state.config.project_data_root, auth_user.id, &name, upload_id);
    fs::create_dir_all(&upload_dir).map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    extract_zip_project(&zip_bytes, &upload_dir)?;

    let project: Project = sqlx::query_as(
        "INSERT INTO projects (user_id, organization_id, workspace_id, name, description, source_type, source_path, local_path, default_branch, git_identity_id)
         VALUES ($1, $2, $3, $4, $5, 'upload', $6, $6, NULL, NULL)
         RETURNING *",
    )
    .bind(auth_user.id)
    .bind(organization_id)
    .bind(workspace_id)
    .bind(&name)
    .bind(&description)
    .bind(&upload_dir)
    .fetch_one(&state.db)
    .await?;
    grant_project_owner(&state, project.id, auth_user.id).await?;

    // ── Vault-seal the raw zip bytes (encrypted backup at DB level) ──────
    // Best-effort: a vault failure must not block the project creation; the
    // extracted files on disk (protected by macOS FileVault at OS level) are
    // the primary working copy for the AI pipeline.
    match state.session_keys.get_cipher(auth_user.id) {
        Some(user_kek) => {
            let vsvc = VaultService::for_user(
                &state.db,
                Arc::new(user_kek),
                auth_user.id,
                state.cipher.clone(),
                auth_user.id,
                None,
            );
            if let Err(e) = vsvc.seal("vault_file", project.id, &zip_bytes).await {
                tracing::warn!(
                    project_id = %project.id,
                    "upload_project: vault seal failed (best-effort — project still created): {}",
                    e
                );
            }
        }
        None => {
            tracing::warn!(
                project_id = %project.id,
                "upload_project: no User KEK in session — vault seal skipped"
            );
        }
    }

    register_project_storage_file(
        &state,
        &project,
        auth_user.id,
        "upload",
        name.clone(),
        upload_dir.clone(),
        Some(zip_bytes.len() as i64),
        "upload",
    )
    .await;

    {
        let db = state.db.clone();
        let proj = project.clone();
        tokio::spawn(async move {
            let _ = rebuild_project_index_all(&db, &proj).await;
        });
    }

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
    require_project_role(&state, id, auth_user.id, "admin").await?;
    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct ArchiveRequest {
    pub archived: bool,
}

/// Phase 4 — soft 封存/取消封存 (sets/clears projects.archived_at).
/// Non-destructive "淡化" toggle: data, todos, meetings all stay; the
/// workspace just renders faded and can be filtered out. Editor role
/// (it's a state change, not a delete).
async fn set_archived(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<ArchiveRequest>,
) -> AppResult<Json<Project>> {
    require_project_role(&state, id, auth_user.id, "editor").await?;
    let project: Project = sqlx::query_as(
        "UPDATE projects
            SET archived_at = CASE WHEN $2 THEN NOW() ELSE NULL END,
                updated_at = NOW()
          WHERE id = $1
          RETURNING *",
    )
    .bind(id)
    .bind(req.archived)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(project))
}

// ── MS-2b: multi-source CRUD ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct NewSourceRequest {
    /// "local" | "git"
    pub kind: String,
    pub source_path: String,
    pub git_identity_id: Option<Uuid>,
    pub default_branch: Option<String>,
    pub label: Option<String>,
}

/// List every source attached to a workspace.
async fn list_sources(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<ProjectSource>>> {
    // viewer access is enough to read the list.
    find_project(&state, id, auth_user.id).await?;
    let sources: Vec<ProjectSource> = sqlx::query_as(
        "SELECT * FROM project_sources WHERE project_id = $1 ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(sources))
}

/// Add a folder or Git repo to a workspace. Git sources are cloned
/// into the project data root; then the whole workspace is reindexed
/// so the new source's files become AI-grounded immediately.
async fn add_source(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<NewSourceRequest>,
) -> AppResult<(StatusCode, Json<ProjectSource>)> {
    require_project_role(&state, id, auth_user.id, "editor").await?;
    let project = find_project(&state, id, auth_user.id).await?;

    let kind = req.kind.trim().to_ascii_lowercase();
    if kind != "local" && kind != "git" {
        return Err(AppError::BadRequest(
            "source kind must be 'local' or 'git'".into(),
        ));
    }
    if req.source_path.trim().is_empty() {
        return Err(AppError::BadRequest("source_path is required".into()));
    }

    let local_path = if kind == "git" {
        let identity = match req.git_identity_id {
            Some(iid) => Some(find_git_identity(&state, iid, auth_user.id).await?),
            None => None,
        };
        let credentials = match identity.as_ref() {
            Some(idn) => Some(identity_credentials(idn, &state, auth_user.id)?),
            None => None,
        };
        let clone_dir = build_clone_dir(
            &state.config.project_data_root,
            auth_user.id,
            req.source_path.trim(),
            Uuid::new_v4(),
        );
        clone_repository(
            req.source_path.trim(),
            &clone_dir,
            credentials.as_ref(),
            req.default_branch.as_deref(),
        )
        .map_err(|e| AppError::Git(e.to_string()))?;
        Some(clone_dir)
    } else {
        None
    };

    let label = req
        .label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            req.source_path
                .trim()
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or("source")
                .to_string()
        });

    let source: ProjectSource = sqlx::query_as(
        "INSERT INTO project_sources
            (project_id, kind, source_path, local_path, git_identity_id, default_branch, label)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING *",
    )
    .bind(id)
    .bind(&kind)
    .bind(req.source_path.trim())
    .bind(&local_path)
    .bind(req.git_identity_id)
    .bind(req.default_branch.as_deref())
    .bind(&label)
    .fetch_one(&state.db)
    .await?;

    // ── Vault-seal the git clone (add_source path) ───────────────────────
    if let Some(clone_dir) = &local_path {
        match state.session_keys.get_cipher(auth_user.id) {
            Some(user_kek) => {
                let dir_path = std::path::Path::new(clone_dir);
                match zip_dir_bytes(dir_path, 100 * 1024 * 1024) {
                    Ok(zip_bytes) => {
                        let vsvc = VaultService::for_user(
                            &state.db,
                            Arc::new(user_kek),
                            auth_user.id,
                            state.cipher.clone(),
                            auth_user.id,
                            None,
                        );
                        if let Err(e) = vsvc.seal("vault_file", source.id, &zip_bytes).await {
                            tracing::warn!(
                                source_id = %source.id,
                                "add_source: vault seal failed (best-effort): {e}"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            source_id = %source.id,
                            "add_source: git repo vault seal skipped: {e}"
                        );
                    }
                }
            }
            None => {
                tracing::warn!(
                    source_id = %source.id,
                    "add_source: no User KEK in session — git vault seal skipped"
                );
            }
        }
    }

    // Reindex in the BACKGROUND. Indexing a large folder embeds every
    // chunk through the local ONNX model — doing it inline made the
    // request hang ("一直在載入中"). The source row is already saved;
    // grounding picks it up as soon as the background reindex finishes.
    {
        let db = state.db.clone();
        let proj = project.clone();
        tokio::spawn(async move {
            let _ = rebuild_project_index_all(&db, &proj).await;
        });
    }

    Ok((StatusCode::CREATED, Json(source)))
}

/// Detach a source from a workspace (row removed; any on-disk clone is
/// intentionally left in place). The workspace is reindexed so the
/// removed source's content stops being AI-grounded.
async fn delete_source(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((id, source_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    require_project_role(&state, id, auth_user.id, "editor").await?;
    let project = find_project(&state, id, auth_user.id).await?;
    sqlx::query("DELETE FROM project_sources WHERE id = $1 AND project_id = $2")
        .bind(source_id)
        .bind(id)
        .execute(&state.db)
        .await?;
    // Background reindex (same reasoning as add_source).
    {
        let db = state.db.clone();
        let proj = project.clone();
        tokio::spawn(async move {
            let _ = rebuild_project_index_all(&db, &proj).await;
        });
    }
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
    require_project_role(&state, id, auth_user.id, "editor").await?;
    let project = find_project(&state, id, auth_user.id).await?;
    // Skip only when there is genuinely nothing to index: a non-code
    // workspace WITHOUT a local folder. Admin/general WITH a folder
    // path is indexable (user wants AI grounded on that folder).
    if project.kind != "code" && project.source_path.trim().is_empty() {
        return Ok(Json(serde_json::json!({
            "indexed_files": 0,
            "skipped": "non-code workspace has no folder to index"
        })));
    }
    let indexed = rebuild_project_index_all(&state.db, &project)
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
    require_project_role(&state, id, auth_user.id, "editor").await?;
    let project = find_project(&state, id, auth_user.id).await?;
    require_git_project(&project)?;
    let root = project_root_path(&project);

    let identity = match project.git_identity_id {
        Some(identity_id) => Some(find_git_identity(&state, identity_id, auth_user.id).await?),
        None => None,
    };
    let credentials = match identity.as_ref() {
        Some(id) => Some(identity_credentials(id, &state, auth_user.id)?),
        None => None,
    };

    checkout_branch(&root, &req.branch, credentials.as_ref())
        .map_err(|e| AppError::Git(e.to_string()))?;

    let updated: Project = sqlx::query_as(
        "UPDATE projects SET default_branch = $1, updated_at = NOW() WHERE id = $2 AND user_can_access_project(id, $3, 'editor') RETURNING *",
    )
    .bind(req.branch.trim())
    .bind(id)
    .bind(auth_user.id)
    .fetch_one(&state.db)
    .await?;

    let _ = rebuild_project_index_all(&state.db, &updated).await;

    Ok(Json(updated))
}

#[derive(Debug, Deserialize)]
pub struct ReactRequest {
    pub query: String,
    pub max_iters: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ReactResponse {
    pub answer: String,
    pub iterations: u32,
    pub steps: Vec<crate::grounding::tools::ToolStep>,
}

/// Get-or-create the single scratch conversation that anchors a
/// (project, user) ReAct session — needed because the grounding
/// provider's firewall audit row FK-references conversations(id).
async fn ensure_react_conversation(
    state: &AppState,
    project_id: Uuid,
    user_id: Uuid,
) -> AppResult<Uuid> {
    if let Some(existing) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM conversations
         WHERE project_id = $1 AND user_id = $2 AND title = '[react]'
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    {
        return Ok(existing);
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO conversations (project_id, user_id, title)
         VALUES ($1, $2, '[react]') RETURNING id",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;
    Ok(id)
}

/// Phase 5 — ReAct tool-calling loop. The model is given a read-only
/// tool set (search_index / read_file / list_tree); it emits
/// `ACTION:` text, the backend executes the tool THROUGH the grounding
/// provider + firewall, feeds the redacted `OBSERVATION:` back, and
/// repeats up to a bounded number of iterations. Initial project
/// context is grounded + firewalled + audited via grounding::assemble.
async fn react_agent(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<ReactRequest>,
) -> AppResult<Json<ReactResponse>> {
    let query = req.query.trim().to_string();
    if query.is_empty() {
        return Err(AppError::BadRequest("query is required".into()));
    }
    let project = find_project(&state, id, auth_user.id).await?;
    let root = project_root_path(&project);
    let credentials =
        crate::grounding::resolve_project_git_credentials(&state.db, state.session_keys.get_cipher(auth_user.id).as_ref(), &project)
            .await;

    // Best-effort pre-grounding sync (Phase 2a) so tools see fresh files.
    let _ = crate::grounding::freshen_local(
        &project,
        crate::grounding::resolve_project_git_credentials(&state.db, state.session_keys.get_cipher(auth_user.id).as_ref(), &project)
            .await,
        &crate::grounding::GroundingSource::default(),
    )
    .await;

    // Initial grounded + firewalled + AUDITED context.
    let conversation_id =
        ensure_react_conversation(&state, project.id, auth_user.id).await?;
    let base_scope = crate::agents::orchestrator::build_project_scope(&project);
    let policy = crate::security::context_firewall::AgentDataPolicy::managed_default();
    let secured = crate::grounding::assemble(crate::grounding::GroundingInputs {
        db: &state.db,
        user_id: auth_user.id,
        project_id: project.id,
        conversation_id,
        mode_label: "react",
        data_policy: &policy,
        base_scope: &base_scope,
        history: &[],
        project_summary: None,
        query: &query,
        project: Some(&project),
        credentials: credentials.as_ref(),
    })
    .await
    .map_err(|e| AppError::Agent(e.to_string()))?;

    let max_iters = req.max_iters.unwrap_or(4).clamp(1, 6);
    let ctx = crate::grounding::tools::ToolCtx {
        db: &state.db,
        project: &project,
        root,
        credentials,
    };

    let mut messages = vec![
        crate::agents::openclaw::ChatMessage {
            role: "system".into(),
            content: format!(
                "{}\n\n===== 已接地的專案脈絡（已過防火牆遮密）=====\n{}",
                crate::grounding::tools::protocol_prompt(),
                secured
                    .project_scope
                    .relevant_file_context
                    .as_deref()
                    .unwrap_or("(無索引脈絡)")
            ),
        },
        crate::agents::openclaw::ChatMessage {
            role: "user".into(),
            content: secured.user_message.clone(),
        },
    ];

    let hermes = crate::agents::hermes::HermesClient::new(&state.config);
    let mut steps: Vec<crate::grounding::tools::ToolStep> = Vec::new();
    let mut answer = String::new();
    let mut iterations = 0u32;

    for _ in 0..max_iters {
        iterations += 1;
        let turn = hermes
            .chat(messages.clone())
            .await
            .map_err(|e| AppError::Agent(e.to_string()))?;

        match crate::grounding::tools::parse_action(&turn) {
            Some(call) => {
                let observation = crate::grounding::tools::run_tool(&ctx, &call).await;
                steps.push(crate::grounding::tools::ToolStep {
                    action: call.name.clone(),
                    args: call.args.clone(),
                    observation_chars: observation.chars().count(),
                });
                messages.push(crate::agents::openclaw::ChatMessage {
                    role: "assistant".into(),
                    content: turn,
                });
                messages.push(crate::agents::openclaw::ChatMessage {
                    role: "user".into(),
                    content: format!(
                        "OBSERVATION:\n{observation}\n\n依據以上觀察繼續。\
若已能回答請輸出 FINAL: <答案>（標明依據檔案），否則再呼叫一個工具。"
                    ),
                });
            }
            None => {
                answer = crate::grounding::tools::strip_final(&turn);
                break;
            }
        }
    }

    if answer.is_empty() {
        answer =
            "(已達最大工具迭代次數，未能產生最終答案；可提高 max_iters 或縮小問題)".into();
    }

    Ok(Json(ReactResponse {
        answer,
        iterations,
        steps,
    }))
}

#[derive(Debug, Deserialize)]
pub struct RemoteFileQuery {
    pub path: String,
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
}

/// Phase 2b — live remote single-file read (GitHub/GitLab Contents
/// API). Reads `path` at `ref` (defaults to the project's default
/// branch / HEAD) straight from the remote, bypassing the local clone.
/// This is the testable surface of the `RemoteLive` grounding source.
async fn get_remote_file(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<RemoteFileQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let project = find_project(&state, id, auth_user.id).await?;
    require_git_project(&project)?;
    let credentials =
        crate::grounding::resolve_project_git_credentials(&state.db, state.session_keys.get_cipher(auth_user.id).as_ref(), &project)
            .await;
    let git_ref = q
        .git_ref
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or(project.default_branch.as_deref())
        .unwrap_or("HEAD")
        .to_string();
    let content =
        crate::grounding::remote_file(&project, credentials.as_ref(), &q.path, &git_ref)
            .await
            .map_err(|e| AppError::Git(e.to_string()))?;
    Ok(Json(serde_json::json!({
        "path": q.path,
        "ref": git_ref,
        "bytes": content.len(),
        "content": content,
    })))
}

async fn sync_git_repo(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    require_project_role(&state, id, auth_user.id, "editor").await?;
    let project = find_project(&state, id, auth_user.id).await?;
    require_git_project(&project)?;
    let root = project_root_path(&project);

    let identity = match project.git_identity_id {
        Some(identity_id) => Some(find_git_identity(&state, identity_id, auth_user.id).await?),
        None => None,
    };
    let credentials = match identity.as_ref() {
        Some(id) => Some(identity_credentials(id, &state, auth_user.id)?),
        None => None,
    };

    let result = sync_current_branch(&root, credentials.as_ref())
        .map_err(|e| AppError::Git(e.to_string()))?;

    let _ = rebuild_project_index_all(&state.db, &project).await;

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
        Some(id) => Some(identity_credentials(id, &state, auth_user.id)?),
        None => None,
    };

    let branches = list_remote_branches(&req.url, credentials.as_ref())
        .await
        .map_err(|e| AppError::Git(e.to_string()))?;

    Ok(Json(serde_json::json!({ "branches": branches })))
}

async fn find_project(state: &AppState, id: Uuid, user_id: Uuid) -> AppResult<Project> {
    let project: Option<Project> = sqlx::query_as(
        "SELECT *, user_project_role(id, $2) AS effective_role FROM projects
         WHERE id = $1 AND user_can_access_project(id, $2, 'viewer')",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    project.ok_or_else(|| AppError::NotFound("Project not found".into()))
}

async fn require_project_role(
    state: &AppState,
    project_id: Uuid,
    user_id: Uuid,
    min_role: &str,
) -> AppResult<()> {
    let allowed: bool = sqlx::query_scalar("SELECT user_can_access_project($1, $2, $3)")
        .bind(project_id)
        .bind(user_id)
        .bind(min_role)
        .fetch_one(&state.db)
        .await?;

    if allowed {
        Ok(())
    } else {
        Err(AppError::NotFound("Project not found".into()))
    }
}

async fn ensure_personal_workspace(state: &AppState, user_id: Uuid) -> AppResult<(Uuid, Uuid)> {
    if let Some(row) = sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT o.id, w.id
         FROM organizations o
         JOIN workspaces w ON w.organization_id = o.id
         WHERE o.owner_user_id = $1
         ORDER BY w.created_at ASC
         LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    {
        return Ok(row);
    }

    let user: (String, String) =
        sqlx::query_as("SELECT email, display_name FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.db)
            .await?;
    let org_name = format!(
        "{} Personal Org",
        if user.1.trim().is_empty() {
            user.0.as_str()
        } else {
            user.1.as_str()
        }
    );
    let org_id: Uuid = sqlx::query_scalar(
        "INSERT INTO organizations (owner_user_id, name) VALUES ($1, $2) RETURNING id",
    )
    .bind(user_id)
    .bind(org_name)
    .fetch_one(&state.db)
    .await?;
    sqlx::query(
        "INSERT INTO organization_members (organization_id, user_id, role)
         VALUES ($1, $2, 'owner') ON CONFLICT DO NOTHING",
    )
    .bind(org_id)
    .bind(user_id)
    .execute(&state.db)
    .await?;
    let workspace_id: Uuid = sqlx::query_scalar(
        "INSERT INTO workspaces (organization_id, name) VALUES ($1, 'Default Workspace') RETURNING id",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role)
         VALUES ($1, $2, 'owner') ON CONFLICT DO NOTHING",
    )
    .bind(workspace_id)
    .bind(user_id)
    .execute(&state.db)
    .await?;
    Ok((org_id, workspace_id))
}

async fn grant_project_owner(state: &AppState, project_id: Uuid, user_id: Uuid) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO project_acl (project_id, user_id, role)
         VALUES ($1, $2, 'owner')
         ON CONFLICT (project_id, user_id) DO UPDATE SET role = 'owner'",
    )
    .bind(project_id)
    .bind(user_id)
    .execute(&state.db)
    .await?;
    Ok(())
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

/// Decrypt a git identity's access token.
///
/// Strict User KEK gate (PR5): normal API paths may decrypt git credentials
/// only with the active in-RAM User KEK. System KEK is reserved for explicit
/// admin recovery tooling and must not be a silent fallback for git/file ops.
fn identity_credentials(
    identity: &GitIdentity,
    state: &AppState,
    user_id: Uuid,
) -> AppResult<GitCredentials> {
    let user_kek = state.session_keys.get_cipher(user_id).ok_or_else(|| {
        AppError::Unauthorized(
            "User KEK session expired — please log in again before using git credentials".into(),
        )
    })?;
    let access_token = user_kek
        .decrypt(&identity.access_token)
        .map_err(|_| {
            AppError::Unauthorized(
                "Git credential is not available through the active User KEK; re-create the identity after login".into(),
            )
        })?;

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

/// Build the on-disk clone directory:
///   `<root>/users/<user_id>/<repo-slug>-<short>`
///
/// Each user gets their own subdirectory so that OS-level `chmod 700`
/// on the per-user directory prevents cross-user filesystem reads even
/// if the kway-svc process account is somehow compromised.
/// Existing projects keep whatever path was stored at create time, so
/// old clones remain accessible after upgrading.
async fn register_project_storage_file(
    state: &AppState,
    project: &Project,
    actor_user_id: Uuid,
    source_type: &str,
    logical_path: String,
    storage_path: String,
    size_bytes: Option<i64>,
    operation: &str,
) {
    let encryption_state = if storage_path.contains(&format!("/users/{}/", project.user_id)) {
        "dmg"
    } else {
        "plaintext_dev"
    };

    match register_workspace_file(
        &state.db,
        RegisterWorkspaceFile {
            owner_user_id: project.user_id,
            organization_id: project.organization_id,
            workspace_id: project.workspace_id,
            project_id: Some(project.id),
            source_type: source_type.to_string(),
            logical_path,
            storage_path,
            classification: "confidential".into(),
            encryption_state: encryption_state.into(),
            content_hash: None,
            size_bytes,
        },
    )
    .await
    {
        Ok(file) => {
            let _ = audit_file_access(
                &state.db,
                file.id,
                Some(actor_user_id),
                operation,
                None,
                None,
                None,
            )
            .await;
        }
        Err(e) => {
            tracing::warn!(
                project_id = %project.id,
                "workspace file registry skipped for project storage: {e}"
            );
        }
    }
}

fn build_clone_dir(root: &str, user_id: Uuid, source_url: &str, clone_id: Uuid) -> String {
    let slug = repo_slug_from_url(source_url);
    let mut short = clone_id.to_string();
    short.retain(|c| c != '-');
    let short = short.chars().take(8).collect::<String>();
    let root = root.trim_end_matches(['/', '\\']);
    format!("{}/users/{}/{}-{}", root, user_id, slug, short)
}

fn build_upload_dir(root: &str, user_id: Uuid, name: &str, upload_id: Uuid) -> String {
    let mut short = upload_id.to_string();
    short.retain(|c| c != '-');
    let short = short.chars().take(8).collect::<String>();
    let slug = safe_slug(name);
    let root = root.trim_end_matches(['/', '\\']);
    format!("{}/users/{}/upload-{}-{}", root, user_id, slug, short)
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

// ── Vault helpers: zip a directory into memory ────────────────────────────────

/// Recursively compute total uncompressed bytes under `dir`.
fn dir_bytes_recursive(dir: &std::path::Path) -> anyhow::Result<u64> {
    let mut total = 0u64;
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_symlink() {
                // Skip symlinks to avoid loops.
            } else if path.is_dir() {
                total += dir_bytes_recursive(&path)?;
            } else if path.is_file() {
                total += fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    Ok(total)
}

/// Recursively add files under `dir` into an open `ZipWriter`.
/// `base` is the root that paths are computed relative to.
fn add_dir_to_zip<W: std::io::Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    base: &std::path::Path,
    dir: &std::path::Path,
    opts: SimpleFileOptions,
) -> anyhow::Result<()> {
    use std::io::Read;
    let mut entries: Vec<_> = fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    // Stable sort so zip contents are deterministic.
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        if path.is_symlink() {
            continue; // skip to prevent traversal loops
        }
        let rel = path
            .strip_prefix(base)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if rel.is_empty() {
            continue;
        }
        if path.is_dir() {
            zip.add_directory(&rel, opts)?;
            add_dir_to_zip(zip, base, &path, opts)?;
        } else if path.is_file() {
            zip.start_file(&rel, opts)?;
            let mut f = fs::File::open(&path)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            std::io::Write::write_all(zip, &buf)?;
        }
    }
    Ok(())
}

/// Zip `dir` into an in-memory `Vec<u8>`.
/// Returns `Err` if the uncompressed directory exceeds `limit_bytes`.
/// This is used for best-effort vault sealing of git clones.
fn zip_dir_bytes(dir: &std::path::Path, limit_bytes: u64) -> anyhow::Result<Vec<u8>> {
    let total = dir_bytes_recursive(dir)?;
    if total > limit_bytes {
        anyhow::bail!(
            "repo too large for vault seal ({} MB uncompressed, limit {} MB) — \
             relying on OS-level FileVault/BitLocker for disk protection",
            total / 1_048_576,
            limit_bytes / 1_048_576,
        );
    }
    let cursor = std::io::Cursor::new(Vec::<u8>::new());
    let mut zip = ZipWriter::new(cursor);
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    add_dir_to_zip(&mut zip, dir, dir, opts)?;
    let inner = zip.finish()?;
    Ok(inner.into_inner())
}
