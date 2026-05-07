use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get},
    Extension, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    db::models::GitIdentity,
    error::{AppError, AppResult},
    git_ops::manager::{list_remote_branches, GitCredentials},
};

async fn validate_git_token(provider: &str, token: &str) -> AppResult<()> {
    let (url, auth_value) = match provider.to_lowercase().as_str() {
        "github" => ("https://api.github.com/user", format!("token {}", token)),
        "gitlab" => (
            "https://gitlab.com/api/v4/user",
            format!("Bearer {}", token),
        ),
        _ => return Ok(()),
    };

    let resp = reqwest::Client::new()
        .get(url)
        .header("Authorization", auth_value)
        .header("User-Agent", "kway-dev-platform/1.0")
        .send()
        .await
        .map_err(|e| AppError::BadRequest(format!("Token validation failed: {}", e)))?;

    match resp.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err(AppError::BadRequest("Token is invalid or expired".into())),
        403 => Err(AppError::BadRequest(
            "Token lacks required permissions (needs repo read access)".into(),
        )),
        s => Err(AppError::BadRequest(format!(
            "Token validation returned HTTP {}",
            s
        ))),
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateGitIdentityRequest {
    pub name: String,
    pub provider: Option<String>,
    pub username: String,
    pub access_token: String,
    pub repository_url: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/git/identities",
            get(list_identities).post(create_identity),
        )
        .route("/git/identities/:id", delete(delete_identity))
}

async fn list_identities(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> AppResult<Json<Vec<GitIdentity>>> {
    let identities: Vec<GitIdentity> =
        sqlx::query_as("SELECT * FROM git_identities WHERE user_id = $1 ORDER BY updated_at DESC")
            .bind(auth_user.id)
            .fetch_all(&state.db)
            .await?;

    Ok(Json(identities))
}

async fn create_identity(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateGitIdentityRequest>,
) -> AppResult<(StatusCode, Json<GitIdentity>)> {
    if req.name.trim().is_empty()
        || req.username.trim().is_empty()
        || req.access_token.trim().is_empty()
    {
        return Err(AppError::BadRequest(
            "name, username and access_token are required".into(),
        ));
    }

    let provider = req
        .provider
        .as_deref()
        .unwrap_or("github")
        .trim()
        .to_lowercase();
    validate_git_token(&provider, req.access_token.trim()).await?;

    if let Some(repository_url) = req
        .repository_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        let credentials = GitCredentials {
            username: req.username.trim().to_string(),
            access_token: req.access_token.trim().to_string(),
        };
        let branches = list_remote_branches(repository_url, Some(&credentials))
            .await
            .map_err(|e| AppError::BadRequest(format!("Repository validation failed: {}", e)))?;
        if branches.is_empty() {
            return Err(AppError::BadRequest(
                "Repository validation failed: no branches found".into(),
            ));
        }
    }

    let identity: GitIdentity = sqlx::query_as(
        "INSERT INTO git_identities (user_id, name, provider, username, access_token)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING *",
    )
    .bind(auth_user.id)
    .bind(req.name.trim())
    .bind(provider)
    .bind(req.username.trim())
    .bind(req.access_token.trim())
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(identity)))
}

async fn delete_identity(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let result = sqlx::query("DELETE FROM git_identities WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(auth_user.id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Git identity not found".into()));
    }

    Ok(StatusCode::NO_CONTENT)
}
