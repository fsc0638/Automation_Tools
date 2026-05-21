//! Vault secrets API — CRUD for user-owned encrypted credentials.
//!
//! All secret *values* are encrypted at rest via the vault (User KEK → DEK).
//! The list and metadata endpoints return only the `vault_secrets` row
//! (label, type, username, url, note) — never the plaintext.
//!
//! The reveal endpoint decrypts and returns the plaintext **once**, with
//! `Cache-Control: no-store` so the browser never caches it.
//!
//! ## Security invariant
//! Every handler rejects requests with no in-RAM User KEK (i.e. the user must
//! be freshly authenticated — not just holding a valid JWT).  This gates vault
//! access on an active login session, not just on a long-lived token.

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    error::{AppError, AppResult},
    security::vault_service::VaultService,
};

// ── Request / Response types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateSecretRequest {
    /// Human-readable label, e.g. "GitHub PAT", "prod DB password".
    pub label: String,
    /// Free-form category tag: "api_key" | "password" | "note" | ...
    pub secret_type: String,
    pub username: Option<String>,
    pub url: Option<String>,
    /// Unencrypted note visible in the metadata listing (don't put secrets here).
    pub note: Option<String>,
    /// The actual secret value — encrypted before being written to the DB.
    pub secret_value: String,
}

/// Metadata returned by list / create.  Never includes the plaintext value.
#[derive(Debug, Serialize, FromRow)]
pub struct SecretMetadata {
    pub id: Uuid,
    pub label: String,
    pub secret_type: String,
    pub username: Option<String>,
    pub url: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct RevealResponse {
    /// The decrypted secret value.  Treat as transient; never persist.
    pub secret_value: String,
}

// ── Routes ────────────────────────────────────────────────────────────────────

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/vault/secrets", get(list_secrets).post(create_secret))
        .route(
            "/vault/secrets/:id",
            delete(delete_secret),
        )
        .route("/vault/secrets/:id/reveal", post(reveal_secret))
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `GET /api/vault/secrets`
///
/// List all vault secret metadata rows owned by the authenticated user.
/// The plaintext secret value is never included.
async fn list_secrets(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> AppResult<Json<Vec<SecretMetadata>>> {
    // Require an active User KEK session (login-gated, not just JWT-gated).
    require_kek(&state, auth_user.id)?;

    let rows: Vec<SecretMetadata> = sqlx::query_as(
        "SELECT id, label, secret_type, username, url, note, created_at, updated_at
         FROM vault_secrets
         WHERE user_id = $1
         ORDER BY created_at DESC",
    )
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}

/// `POST /api/vault/secrets`
///
/// Create a new vault secret.  The `secret_value` field is encrypted with the
/// User KEK before being written; the plaintext never touches the DB.
async fn create_secret(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateSecretRequest>,
) -> AppResult<Json<SecretMetadata>> {
    if req.label.trim().is_empty() {
        return Err(AppError::BadRequest("label is required".into()));
    }
    if req.secret_value.is_empty() {
        return Err(AppError::BadRequest("secret_value is required".into()));
    }

    let user_kek = require_kek(&state, auth_user.id)?;

    // Insert the metadata row first to obtain the stable `id` used as object_id.
    let meta: SecretMetadata = sqlx::query_as(
        "INSERT INTO vault_secrets (user_id, label, secret_type, username, url, note)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, label, secret_type, username, url, note, created_at, updated_at",
    )
    .bind(auth_user.id)
    .bind(req.label.trim())
    .bind(&req.secret_type)
    .bind(req.username.as_deref())
    .bind(req.url.as_deref())
    .bind(req.note.as_deref())
    .fetch_one(&state.db)
    .await?;

    // Seal the plaintext into vault_ciphertexts / vault_key_wrappings.
    let vsvc = VaultService::for_user(
        &state.db,
        Arc::new(user_kek),
        auth_user.id,
        state.cipher.clone(),
        auth_user.id,
        None, // ip_addr: extend later with ConnectInfo
    );

    if let Err(e) = vsvc.seal("vault_secret", meta.id, req.secret_value.as_bytes()).await {
        // Seal failed — roll back the metadata row so we don't leave an
        // unencryptable orphan in vault_secrets.
        let _ = sqlx::query("DELETE FROM vault_secrets WHERE id = $1")
            .bind(meta.id)
            .execute(&state.db)
            .await;
        return Err(AppError::Internal(anyhow::anyhow!("vault seal failed: {}", e)));
    }

    Ok(Json(meta))
}

/// `POST /api/vault/secrets/:id/reveal`
///
/// Decrypt and return the secret value once.  Every call is audit-logged.
/// The response includes `Cache-Control: no-store` to prevent browser caching.
async fn reveal_secret(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    // Ownership check: ensure the secret belongs to the requesting user.
    let owner: Option<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM vault_secrets WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;

    match owner {
        None => return Err(AppError::NotFound("Secret not found".into())),
        Some(uid) if uid != auth_user.id => {
            return Err(AppError::Forbidden("Not your secret".into()));
        }
        _ => {}
    }

    let user_kek = require_kek(&state, auth_user.id)?;

    let vsvc = VaultService::for_user(
        &state.db,
        Arc::new(user_kek),
        auth_user.id,
        state.cipher.clone(),
        auth_user.id,
        None,
    );

    let plaintext_bytes = vsvc
        .open("vault_secret", id, "reveal")
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("vault open failed: {}", e)))?;

    let secret_value = String::from_utf8(plaintext_bytes)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("vault: decrypted value is not UTF-8")))?;

    // Prevent the browser / any HTTP intermediary from caching the plaintext.
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        "no-store, no-cache, must-revalidate".parse().unwrap(),
    );
    headers.insert(header::PRAGMA, "no-cache".parse().unwrap());

    Ok((StatusCode::OK, headers, Json(RevealResponse { secret_value })))
}

/// `DELETE /api/vault/secrets/:id`
///
/// Hard-delete the metadata row and all vault material (ciphertext + wrappings).
/// This is an irreversible operation; the plaintext is gone.
async fn delete_secret(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    // Ownership check.
    let owner: Option<Uuid> =
        sqlx::query_scalar("SELECT user_id FROM vault_secrets WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;

    match owner {
        None => return Err(AppError::NotFound("Secret not found".into())),
        Some(uid) if uid != auth_user.id => {
            return Err(AppError::Forbidden("Not your secret".into()));
        }
        _ => {}
    }

    let user_kek = require_kek(&state, auth_user.id)?;

    let vsvc = VaultService::for_user(
        &state.db,
        Arc::new(user_kek),
        auth_user.id,
        state.cipher.clone(),
        auth_user.id,
        None,
    );

    // Purge vault material first; then delete the metadata row.
    // If purge fails we keep the metadata row so nothing is left in an
    // undeleted-but-unreadable limbo.
    vsvc.purge("vault_secret", id)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("vault purge failed: {}", e)))?;

    sqlx::query("DELETE FROM vault_secrets WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Extract the User KEK `TokenCipher` for `user_id` from the session store.
/// Returns `401 Unauthorized` with a clear message if the session has expired
/// or was never established (user has a JWT but has not logged in this process).
fn require_kek(
    state: &AppState,
    user_id: Uuid,
) -> AppResult<crate::crypto::TokenCipher> {
    state.session_keys.get_cipher(user_id).ok_or_else(|| {
        AppError::Unauthorized(
            "Vault session expired — please log in again to access vault secrets".into(),
        )
    })
}
