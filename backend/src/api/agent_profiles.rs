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
    pub labels: Option<Vec<String>>,
    pub allowed_classification_max: Option<String>,
    pub allow_code_context: Option<bool>,
    pub allow_project_memory: Option<bool>,
    pub allow_conversation_history: Option<bool>,
    pub require_redaction: Option<bool>,
    pub external_processing_allowed: Option<bool>,
    pub retention_policy: Option<String>,
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
    pub labels: Option<Vec<String>>,
    pub allowed_classification_max: Option<String>,
    pub allow_code_context: Option<bool>,
    pub allow_project_memory: Option<bool>,
    pub allow_conversation_history: Option<bool>,
    pub require_redaction: Option<bool>,
    pub external_processing_allowed: Option<bool>,
    pub retention_policy: Option<String>,
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
    let profiles: Vec<AgentProfile> =
        sqlx::query_as("SELECT * FROM agent_profiles WHERE user_id = $1 ORDER BY updated_at DESC")
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
        return Err(AppError::BadRequest(
            "name, model and api_key are required".into(),
        ));
    }

    let encrypted_key = state
        .cipher
        .encrypt(api_key)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("agent key encryption failed: {}", e)))?;

    let labels = req.labels.unwrap_or_default();
    let allowed_classification_max = normalize_classification(
        req.allowed_classification_max
            .as_deref()
            .unwrap_or("confidential"),
    )?;
    let retention_policy = normalize_retention_policy(
        req.retention_policy
            .as_deref()
            .unwrap_or("provider_default"),
    )?;
    let profile: AgentProfile = sqlx::query_as(
        "INSERT INTO agent_profiles
         (user_id, name, provider, model, base_url, role_prompt, api_key, enabled, labels,
          allowed_classification_max, allow_code_context, allow_project_memory,
          allow_conversation_history, require_redaction, external_processing_allowed, retention_policy)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
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
    .bind(&labels)
    .bind(allowed_classification_max)
    .bind(req.allow_code_context.unwrap_or(true))
    .bind(req.allow_project_memory.unwrap_or(true))
    .bind(req.allow_conversation_history.unwrap_or(true))
    .bind(req.require_redaction.unwrap_or(true))
    .bind(req.external_processing_allowed.unwrap_or(true))
    .bind(retention_policy)
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
    let existing: AgentProfile =
        sqlx::query_as("SELECT * FROM agent_profiles WHERE id = $1 AND user_id = $2")
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

    let api_key = match req
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(value) => state.cipher.encrypt(value).map_err(|e| {
            AppError::Internal(anyhow::anyhow!("agent key encryption failed: {}", e))
        })?,
        None => existing.api_key,
    };

    let labels = req.labels.unwrap_or(existing.labels);
    let allowed_classification_max = match req.allowed_classification_max.as_deref() {
        Some(value) => normalize_classification(value)?,
        None => existing.allowed_classification_max.as_str(),
    };
    let retention_policy = match req.retention_policy.as_deref() {
        Some(value) => normalize_retention_policy(value)?,
        None => existing.retention_policy.as_str(),
    };
    let profile: AgentProfile = sqlx::query_as(
        "UPDATE agent_profiles
         SET name = $3, provider = $4, model = $5, base_url = $6,
             role_prompt = $7, api_key = $8, enabled = $9, labels = $10,
             allowed_classification_max = $11, allow_code_context = $12,
             allow_project_memory = $13, allow_conversation_history = $14,
             require_redaction = $15, external_processing_allowed = $16,
             retention_policy = $17, updated_at = NOW()
         WHERE id = $1 AND user_id = $2
         RETURNING *",
    )
    .bind(id)
    .bind(auth_user.id)
    .bind(name)
    .bind(provider)
    .bind(model)
    .bind(
        req.base_url
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or(existing.base_url),
    )
    .bind(req.role_prompt.unwrap_or(existing.role_prompt))
    .bind(api_key)
    .bind(req.enabled.unwrap_or(existing.enabled))
    .bind(&labels)
    .bind(allowed_classification_max)
    .bind(
        req.allow_code_context
            .unwrap_or(existing.allow_code_context),
    )
    .bind(
        req.allow_project_memory
            .unwrap_or(existing.allow_project_memory),
    )
    .bind(
        req.allow_conversation_history
            .unwrap_or(existing.allow_conversation_history),
    )
    .bind(req.require_redaction.unwrap_or(existing.require_redaction))
    .bind(
        req.external_processing_allowed
            .unwrap_or(existing.external_processing_allowed),
    )
    .bind(retention_policy)
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

fn normalize_classification(value: &str) -> AppResult<&'static str> {
    match value.trim().to_lowercase().as_str() {
        "public" => Ok("public"),
        "internal" => Ok("internal"),
        "confidential" => Ok("confidential"),
        "restricted" => Ok("restricted"),
        "secret" => Ok("secret"),
        _ => Err(AppError::BadRequest(
            "allowed_classification_max must be public, internal, confidential, restricted, or secret".into(),
        )),
    }
}

fn normalize_retention_policy(value: &str) -> AppResult<&'static str> {
    match value.trim().to_lowercase().as_str() {
        "none" => Ok("none"),
        "session" => Ok("session"),
        "provider_default" | "provider-default" | "default" => Ok("provider_default"),
        _ => Err(AppError::BadRequest(
            "retention_policy must be none, session, or provider_default".into(),
        )),
    }
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}
