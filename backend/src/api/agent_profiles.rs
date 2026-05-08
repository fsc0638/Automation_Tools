use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, patch},
    Extension, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    db::models::AgentProfile,
    error::{AppError, AppResult},
};

#[derive(Debug, Deserialize)]
pub struct CreateAgentProfileRequest {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub base_url: Option<String>,
    pub role_prompt: Option<String>,
    pub api_key: String,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAgentProfileRequest {
    pub name: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub role_prompt: Option<String>,
    pub api_key: Option<String>,
    pub enabled: Option<bool>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agents", get(list_profiles).post(create_profile))
        .route("/agents/:id", patch(update_profile).delete(delete_profile))
}

async fn list_profiles(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> AppResult<Json<Vec<AgentProfile>>> {
    let profiles: Vec<AgentProfile> = sqlx::query_as(
        "SELECT * FROM agent_profiles WHERE user_id = $1 ORDER BY updated_at DESC",
    )
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(profiles))
}

async fn create_profile(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateAgentProfileRequest>,
) -> AppResult<(StatusCode, Json<AgentProfile>)> {
    let provider = normalize_provider(&req.provider)?;
    let name = req.name.trim();
    let model = req.model.trim();
    let api_key = req.api_key.trim();
    if name.is_empty() || model.is_empty() || api_key.is_empty() {
        return Err(AppError::BadRequest("name, model and api_key are required".into()));
    }

    let encrypted_key = state
        .cipher
        .encrypt(api_key)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("agent key encryption failed: {}", e)))?;

    let profile: AgentProfile = sqlx::query_as(
        "INSERT INTO agent_profiles (user_id, name, provider, model, base_url, role_prompt, api_key, enabled)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING *",
    )
    .bind(auth_user.id)
    .bind(name)
    .bind(provider)
    .bind(model)
    .bind(clean_optional(req.base_url.as_deref()))
    .bind(req.role_prompt.unwrap_or_default())
    .bind(encrypted_key)
    .bind(req.enabled.unwrap_or(true))
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(profile)))
}

async fn update_profile(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateAgentProfileRequest>,
) -> AppResult<Json<AgentProfile>> {
    let existing: AgentProfile = sqlx::query_as(
        "SELECT * FROM agent_profiles WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(auth_user.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Agent profile not found".into()))?;

    let provider = match req.provider.as_deref() {
        Some(value) => normalize_provider(value)?,
        None => existing.provider.as_str(),
    };
    let name = req.name.as_deref().unwrap_or(&existing.name).trim();
    let model = req.model.as_deref().unwrap_or(&existing.model).trim();
    if name.is_empty() || model.is_empty() {
        return Err(AppError::BadRequest("name and model are required".into()));
    }

    let api_key = match req.api_key.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(value) => state
            .cipher
            .encrypt(value)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("agent key encryption failed: {}", e)))?,
        None => existing.api_key,
    };

    let profile: AgentProfile = sqlx::query_as(
        "UPDATE agent_profiles
         SET name = $3, provider = $4, model = $5, base_url = $6,
             role_prompt = $7, api_key = $8, enabled = $9, updated_at = NOW()
         WHERE id = $1 AND user_id = $2
         RETURNING *",
    )
    .bind(id)
    .bind(auth_user.id)
    .bind(name)
    .bind(provider)
    .bind(model)
    .bind(req.base_url.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string).or(existing.base_url))
    .bind(req.role_prompt.unwrap_or(existing.role_prompt))
    .bind(api_key)
    .bind(req.enabled.unwrap_or(existing.enabled))
    .fetch_one(&state.db)
    .await?;

    Ok(Json(profile))
}

async fn delete_profile(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let result = sqlx::query("DELETE FROM agent_profiles WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(auth_user.id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Agent profile not found".into()));
    }

    Ok(StatusCode::NO_CONTENT)
}

fn normalize_provider(value: &str) -> AppResult<&'static str> {
    match value.trim().to_lowercase().as_str() {
        "openai" => Ok("openai"),
        "openai_compatible" | "openai-compatible" | "compatible" => Ok("openai_compatible"),
        "gemini" | "google" => Ok("gemini"),
        "anthropic" | "claude" => Ok("anthropic"),
        _ => Err(AppError::BadRequest(
            "provider must be openai, openai_compatible, gemini, or anthropic".into(),
        )),
    }
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}
