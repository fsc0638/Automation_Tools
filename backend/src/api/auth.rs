use axum::{
    extract::{Query, Request, State},
    http::header::AUTHORIZATION,
    middleware::Next,
    response::Response,
    routing::{get, post},
    Extension, Json, Router,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use std::sync::Arc;

use crate::{
    api::AppState,
    crypto::TokenCipher,
    db::models::User,
    error::{AppError, AppResult},
    security::{
        dmg_manager,
        vault_crypto::{ARGON2_M_COST, ARGON2_OUTPUT_LEN, ARGON2_P_COST, ARGON2_T_COST,
                       AUTH_DOMAIN, KEK_DOMAIN},
        vault_service::VaultService,
    },
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user id
    pub email: String,
    pub exp: i64,
    pub iat: i64,
}

/// Register payload (client-held KEK protocol, mig 0049).
///
/// All three of `kek_salt`, `auth_hash`, `user_kek` are derived in the
/// browser / iOS app from the plaintext password — the password itself
/// never leaves the device. Each field is base64 of exactly 32 random
/// or Argon2id-derived bytes.
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub display_name: String,
    pub kek_salt: String,
    pub auth_hash: String,
    pub user_kek: String,
}

/// Login payload (client-held KEK protocol).
///
/// `auth_hash` proves identity (server compares its server-side
/// Argon2 hash against `users.password_hash`).  `user_kek` is held in
/// the in-RAM session store and used to wrap/unwrap vault DEKs for
/// the session duration.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub auth_hash: String,
    pub user_kek: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    /// Refresh-token lifetime in seconds. Frontend uses this to decide
    /// when to proactively rotate; the canonical authority is still the
    /// hash stored server-side.
    pub refresh_expires_in: i64,
    pub user: UserInfo,
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Serialize)]
pub struct RefreshResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub refresh_expires_in: i64,
}

#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
}

/// Public response of `GET /auth/kek-params` — tells the client
/// exactly how to derive `auth_hash` and `user_kek` so they match
/// what the server expects.
#[derive(Debug, Serialize)]
pub struct KekParamsResponse {
    pub kek_salt: String,
    pub argon2_m_cost: u32,
    pub argon2_t_cost: u32,
    pub argon2_p_cost: u32,
    pub argon2_output_len: u32,
    pub kek_domain: String,
    pub auth_domain: String,
}

#[derive(Debug, Deserialize)]
pub struct KekParamsQuery {
    pub email: String,
}

#[derive(Debug, Deserialize)]
struct ChangePasswordRequest {
    current_auth_hash: String,
    new_auth_hash: String,
    current_user_kek: String,
    new_user_kek: String,
    /// Optional: rotate the per-user kek_salt at the same time. Normally
    /// omitted — keep the existing salt so derived bytes stay stable
    /// across devices.
    new_kek_salt: Option<String>,
}

// ── DMG helpers ───────────────────────────────────────────────────────────────

/// After a successful login or registration, ensure the user's encrypted
/// disk image is mounted at `<project_data_root>/users/<user_id>/`.
///
/// First call: generates a random 32-byte passphrase, seals it in the
/// vault, creates the sparse image, then mounts it.
/// Subsequent calls: retrieves the passphrase from vault, mounts (idempotent).
///
/// Best-effort: errors are logged with `warn!` but never fail the login.
/// The user can still access the platform; only on-disk data is unencrypted.
async fn dmg_on_login(state: &AppState, user_id: Uuid, user_kek: [u8; 32]) {
    let Some(ref dmg_root) = state.config.dmg_root else {
        return; // DMG encryption not configured — skip silently
    };

    let vsvc = VaultService::for_user(
        &state.db,
        Arc::new(TokenCipher::from_raw_key(&user_kek)),
        user_id,
        state.cipher.clone(),
        user_id,
        None,
    );

    // Retrieve or create the DMG passphrase.
    let raw_key: Vec<u8> = if vsvc.exists("user_dmg_key", user_id).await {
        match vsvc.open("user_dmg_key", user_id, "dmg_mount").await {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!(user_id = %user_id, "dmg_on_login: vault open error: {e:#}");
                return;
            }
        }
    } else {
        // First login — generate and seal the passphrase.
        use rand::RngCore;
        let mut key = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        if let Err(e) = vsvc.seal("user_dmg_key", user_id, &key).await {
            tracing::warn!(user_id = %user_id, "dmg_on_login: vault seal error: {e:#}");
            return;
        }
        key
    };

    // Capture values for the blocking thread.
    let dmg_root2 = dmg_root.clone();
    let data_root2 = state.config.project_data_root.clone();
    let size_mb = state.config.dmg_size_mb;
    let image_exists = dmg_manager::image_exists(&dmg_root2, user_id);

    let result = tokio::task::spawn_blocking(move || {
        if !image_exists {
            dmg_manager::create(&dmg_root2, size_mb, user_id, &raw_key)?;
            tracing::info!(user_id = %user_id, "dmg: created new encrypted image");
        }
        dmg_manager::mount(&dmg_root2, &data_root2, user_id, &raw_key)?;
        tracing::info!(user_id = %user_id, "dmg: mounted at users/{user_id}/");
        anyhow::Ok(())
    })
    .await;

    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!(user_id = %user_id, "dmg_on_login: hdiutil error: {e:#}"),
        Err(e) => tracing::warn!(user_id = %user_id, "dmg_on_login: task panic: {e}"),
    }
}

/// On logout, detach the user's encrypted disk image.
/// Best-effort — never fails the logout.
async fn dmg_on_logout(state: &AppState, user_id: Uuid) {
    let Some(_) = state.config.dmg_root else {
        return;
    };

    let data_root = state.config.project_data_root.clone();
    let result = tokio::task::spawn_blocking(move || dmg_manager::unmount(&data_root, user_id))
        .await;

    match result {
        Ok(Ok(())) => tracing::info!(user_id = %user_id, "dmg: unmounted"),
        Ok(Err(e)) => tracing::warn!(user_id = %user_id, "dmg_on_logout: {e:#}"),
        Err(e) => tracing::warn!(user_id = %user_id, "dmg_on_logout: task panic: {e}"),
    }
}

// ── Route builders ────────────────────────────────────────────────────────────

pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/auth/kek-params", get(kek_params))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
}

/// Routes that require an authenticated session (JWT via `require_auth`).
pub fn protected_routes() -> Router<AppState> {
    Router::new().route("/auth/change-password", post(change_password))
}

// ── Token helpers ─────────────────────────────────────────────────────────────

/// Generate a refresh token: 32 cryptographically random bytes encoded
/// as URL-safe base64. The server stores only the SHA-256 hash, so even
/// a DB leak doesn't expose live sessions.
fn generate_refresh_token() -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_refresh_token(token: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(token.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

/// Insert a refresh token row keyed by hash; returns the raw token.
async fn issue_refresh_token(state: &AppState, user_id: Uuid) -> AppResult<(String, i64)> {
    let raw = generate_refresh_token();
    let hash = hash_refresh_token(&raw);
    let ttl_days = state.config.refresh_token_expiry_days;
    let expires_at = Utc::now() + Duration::days(ttl_days);

    sqlx::query(
        "INSERT INTO refresh_tokens (user_id, token_hash, expires_at)
         VALUES ($1, $2, $3)",
    )
    .bind(user_id)
    .bind(&hash)
    .bind(expires_at)
    .execute(&state.db)
    .await?;

    // Opportunistically prune expired rows for this user so the table
    // doesn't grow forever. Best-effort; ignore errors.
    let _ = sqlx::query(
        "DELETE FROM refresh_tokens
         WHERE user_id = $1 AND expires_at < NOW()",
    )
    .bind(user_id)
    .execute(&state.db)
    .await;

    Ok((raw, ttl_days * 86400))
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// Normalise an email for stable indexing into anti-enumeration HMAC.
fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Deterministic fake kek_salt for unknown emails.
///
/// Returns `HMAC-SHA256(JWT_SECRET, "kek-salt::" || normalize(email))[..32]`.
/// A request for a non-existent account is indistinguishable from a real one:
/// same shape, same Argon2 params, same-looking salt. Without `JWT_SECRET`
/// an attacker can't precompute these, so probing the endpoint reveals no
/// information about which emails are registered.
fn fake_kek_salt(jwt_secret: &str, email: &str) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(jwt_secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(b"kek-salt::");
    mac.update(normalize_email(email).as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// Decode a base64-encoded 32-byte value (kek_salt / auth_hash / user_kek).
fn decode_32(label: &str, b64: &str) -> AppResult<[u8; 32]> {
    let bytes = B64
        .decode(b64.trim())
        .map_err(|e| AppError::BadRequest(format!("{label}: invalid base64: {e}")))?;
    bytes
        .try_into()
        .map_err(|v: Vec<u8>| AppError::BadRequest(format!("{label}: expected 32 bytes, got {}", v.len())))
}

/// Public endpoint: returns the kek_salt + Argon2 contract a client
/// needs to derive `auth_hash` and `user_kek` before calling /auth/login.
///
/// To defeat email enumeration, unknown emails get a deterministic fake
/// salt derived from JWT_SECRET — same shape, indistinguishable from a
/// real reply. The client always proceeds to derive + attempt login;
/// only `/auth/login` reveals whether the account actually exists.
async fn kek_params(
    State(state): State<AppState>,
    Query(q): Query<KekParamsQuery>,
) -> AppResult<Json<KekParamsResponse>> {
    if q.email.trim().is_empty() {
        return Err(AppError::BadRequest("email is required".into()));
    }

    let normalized = normalize_email(&q.email);
    let row: Option<(Vec<u8>,)> =
        sqlx::query_as("SELECT kek_salt FROM users WHERE LOWER(email) = $1")
            .bind(&normalized)
            .fetch_optional(&state.db)
            .await?;

    let kek_salt = row
        .map(|(b,)| b)
        .unwrap_or_else(|| fake_kek_salt(&state.config.jwt_secret, &q.email));

    Ok(Json(KekParamsResponse {
        kek_salt: B64.encode(&kek_salt),
        argon2_m_cost: ARGON2_M_COST,
        argon2_t_cost: ARGON2_T_COST,
        argon2_p_cost: ARGON2_P_COST,
        argon2_output_len: ARGON2_OUTPUT_LEN,
        kek_domain: std::str::from_utf8(KEK_DOMAIN).unwrap().to_string(),
        auth_domain: std::str::from_utf8(AUTH_DOMAIN).unwrap().to_string(),
    }))
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> AppResult<Json<AuthResponse>> {
    if req.email.trim().is_empty() {
        return Err(AppError::BadRequest("email is required".into()));
    }

    let kek_salt = decode_32("kek_salt", &req.kek_salt)?;
    let auth_hash = decode_32("auth_hash", &req.auth_hash)?;
    let user_kek = decode_32("user_kek", &req.user_kek)?;

    let existing: Option<User> = sqlx::query_as("SELECT * FROM users WHERE email = $1")
        .bind(&req.email)
        .fetch_optional(&state.db)
        .await?;
    if existing.is_some() {
        return Err(AppError::BadRequest("Email already registered".into()));
    }

    // Server-side Argon2 over the client-derived auth_hash. A DB leak still
    // costs the attacker an offline brute-force over the *32-byte auth_hash*
    // space (effectively un-breakable), not over passwords.
    let auth_hash_b64 = B64.encode(auth_hash);
    let password_hash = tokio::task::spawn_blocking(move || hash_password(&auth_hash_b64))
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("spawn_blocking panic: {}", e)))??;

    let user: User = sqlx::query_as(
        "INSERT INTO users (email, password_hash, display_name, kek_salt)
         VALUES ($1, $2, $3, $4)
         RETURNING *",
    )
    .bind(&req.email)
    .bind(&password_hash)
    .bind(&req.display_name)
    .bind(&kek_salt[..])
    .fetch_one(&state.db)
    .await?;

    state.session_keys.insert(user.id, user_kek);
    // Provision and mount the per-user encrypted disk image (macOS, best-effort).
    dmg_on_login(&state, user.id, user_kek).await;

    let access =
        generate_token(&user, &state.config.jwt_secret, state.config.jwt_expiry_hours)?;
    let (refresh_token, refresh_expires_in) = issue_refresh_token(&state, user.id).await?;

    Ok(Json(AuthResponse {
        access_token: access,
        refresh_token,
        token_type: "Bearer".into(),
        refresh_expires_in,
        user: UserInfo {
            id: user.id,
            email: user.email,
            display_name: user.display_name,
        },
    }))
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<AuthResponse>> {
    let auth_hash = decode_32("auth_hash", &req.auth_hash)?;
    let user_kek = decode_32("user_kek", &req.user_kek)?;

    let user: Option<User> = sqlx::query_as("SELECT * FROM users WHERE email = $1")
        .bind(&req.email)
        .fetch_optional(&state.db)
        .await?;

    // Always run a verify_password call (even on missing user) to keep the
    // timing profile constant — otherwise the response time leaks whether
    // the email exists.
    let (verified, user) = match user {
        Some(u) => {
            let auth_hash_b64 = B64.encode(auth_hash);
            let pw_hash = u.password_hash.clone();
            let ok = tokio::task::spawn_blocking(move || verify_password(&auth_hash_b64, &pw_hash))
                .await
                .map_err(|e| AppError::Internal(anyhow::anyhow!("spawn_blocking panic: {}", e)))??;
            (ok, Some(u))
        }
        None => {
            // Dummy verify against a throw-away hash so timing matches the
            // success path. The result is ignored.
            let dummy_hash = hash_password("dummy-input-for-timing")?;
            let _ = tokio::task::spawn_blocking(move || verify_password("dummy-input", &dummy_hash))
                .await
                .map_err(|e| AppError::Internal(anyhow::anyhow!("spawn_blocking panic: {}", e)))??;
            (false, None)
        }
    };

    if !verified {
        return Err(AppError::Unauthorized("Invalid credentials".into()));
    }
    let user = user.expect("verified=true implies user exists");

    state.session_keys.insert(user.id, user_kek);
    dmg_on_login(&state, user.id, user_kek).await;

    let access =
        generate_token(&user, &state.config.jwt_secret, state.config.jwt_expiry_hours)?;
    let (refresh_token, refresh_expires_in) = issue_refresh_token(&state, user.id).await?;

    Ok(Json(AuthResponse {
        access_token: access,
        refresh_token,
        token_type: "Bearer".into(),
        refresh_expires_in,
        user: UserInfo {
            id: user.id,
            email: user.email,
            display_name: user.display_name,
        },
    }))
}

async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> AppResult<Json<RefreshResponse>> {
    let hash = hash_refresh_token(&req.refresh_token);

    // Atomically delete the presented refresh token and capture its
    // user_id only if it's not expired. This implements *rotation*:
    // each refresh consumes the old token, so a leaked token is one-use.
    let row: Option<(Uuid,)> = sqlx::query_as(
        "DELETE FROM refresh_tokens
         WHERE token_hash = $1 AND expires_at > NOW()
         RETURNING user_id",
    )
    .bind(&hash)
    .fetch_optional(&state.db)
    .await?;
    let user_id = row
        .ok_or_else(|| AppError::Unauthorized("Invalid or expired refresh token".into()))?
        .0;

    let user: User = sqlx::query_as("SELECT * FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::Unauthorized("User no longer exists".into()))?;

    // Extend the in-RAM User KEK TTL so active users don't need to log in
    // again just because their session key expired mid-session.
    state.session_keys.refresh(user_id);

    let access =
        generate_token(&user, &state.config.jwt_secret, state.config.jwt_expiry_hours)?;
    let (refresh_token, refresh_expires_in) = issue_refresh_token(&state, user.id).await?;

    Ok(Json(RefreshResponse {
        access_token: access,
        refresh_token,
        token_type: "Bearer".into(),
        refresh_expires_in,
    }))
}

async fn logout(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> AppResult<axum::http::StatusCode> {
    let hash = hash_refresh_token(&req.refresh_token);

    // Use RETURNING to get the user_id so we can evict the in-RAM User KEK.
    // If the token was already expired/missing this is a no-op (None).
    let row: Option<(Uuid,)> = sqlx::query_as(
        "DELETE FROM refresh_tokens WHERE token_hash = $1 RETURNING user_id",
    )
    .bind(&hash)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    if let Some((user_id,)) = row {
        // Detach the encrypted disk image before evicting the KEK (best-effort).
        dmg_on_logout(&state, user_id).await;
        // Zeroizes the 32-byte KEK in RAM via SessionEntry::Drop.
        state.session_keys.remove(user_id);
    }

    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Change password (client-held KEK protocol).
///
/// The client derives both old and new auth_hash + user_kek from the
/// respective plaintext passwords, then sends all four to the server.
/// The server:
///   1. verifies current_auth_hash against `users.password_hash`,
///   2. unwraps every `user:<id>` DEK with current_user_kek, re-wraps
///      with new_user_kek,
///   3. stores Argon2(new_auth_hash) as the new password_hash,
///   4. (optionally) rotates kek_salt,
///   5. swaps the in-RAM session KEK.
///
/// All DB writes happen in one transaction. The System KEK wrappings
/// are untouched — same DEK bytes regardless of which user KEK wraps
/// the user's copy.
async fn change_password(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<ChangePasswordRequest>,
) -> AppResult<axum::http::StatusCode> {
    let current_auth_hash = decode_32("current_auth_hash", &req.current_auth_hash)?;
    let new_auth_hash = decode_32("new_auth_hash", &req.new_auth_hash)?;
    let current_user_kek = decode_32("current_user_kek", &req.current_user_kek)?;
    let new_user_kek = decode_32("new_user_kek", &req.new_user_kek)?;
    let new_kek_salt = req
        .new_kek_salt
        .as_deref()
        .map(|s| decode_32("new_kek_salt", s))
        .transpose()?;

    let user: User = sqlx::query_as("SELECT * FROM users WHERE id = $1")
        .bind(auth_user.id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::Unauthorized("User not found".into()))?;

    // Verify current credentials + hash the new auth_hash in one blocking call.
    let cur_b64 = B64.encode(current_auth_hash);
    let new_b64 = B64.encode(new_auth_hash);
    let pw_hash = user.password_hash.clone();

    let result = tokio::task::spawn_blocking(move || {
        let ok = verify_password(&cur_b64, &pw_hash)?;
        if !ok {
            return Ok::<Option<String>, AppError>(None);
        }
        Ok(Some(hash_password(&new_b64)?))
    })
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("spawn_blocking panic: {}", e)))??;

    let new_hash = result
        .ok_or_else(|| AppError::Unauthorized("Current password is incorrect".into()))?;

    re_wrap_deks_and_update_password(
        &state.db,
        auth_user.id,
        &current_user_kek,
        &new_user_kek,
        &new_hash,
        new_kek_salt.as_ref(),
    )
    .await?;

    state.session_keys.insert(auth_user.id, new_user_kek);

    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Re-wrap every `vault_key_wrappings` row keyed by `user:{id}` from the old
/// User KEK to the new one, update `users.password_hash` (and optionally
/// `kek_salt`) — all in one atomic transaction so a crash mid-flight leaves
/// the DB consistent.
async fn re_wrap_deks_and_update_password(
    db: &sqlx::PgPool,
    user_id: Uuid,
    old_kek: &[u8; 32],
    new_kek: &[u8; 32],
    new_hash: &str,
    new_kek_salt: Option<&[u8; 32]>,
) -> AppResult<()> {
    use crate::crypto::TokenCipher;
    use crate::security::vault_crypto::{unwrap_dek, wrap_dek};

    let old_cipher = TokenCipher::from_raw_key(old_kek);
    let new_cipher = TokenCipher::from_raw_key(new_kek);
    let alias = format!("user:{}", user_id);

    let mut tx = db.begin().await?;

    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT object_id, wrapped_dek
         FROM vault_key_wrappings
         WHERE kek_alias = $1
         FOR UPDATE",
    )
    .bind(&alias)
    .fetch_all(&mut *tx)
    .await?;

    for (object_id, wrapped_dek) in rows {
        let dek = unwrap_dek(&old_cipher, &wrapped_dek)
            .map_err(|e| AppError::Unauthorized(format!(
                "re-wrap unwrap object_id={object_id}: current_user_kek does not match stored wrapping ({e})"
            )))?;
        let new_wrapped = wrap_dek(&new_cipher, &dek)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("re-wrap wrap object_id={}: {}", object_id, e)))?;

        sqlx::query(
            "UPDATE vault_key_wrappings
             SET wrapped_dek = $1
             WHERE object_id = $2 AND kek_alias = $3",
        )
        .bind(&new_wrapped)
        .bind(object_id)
        .bind(&alias)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(new_hash)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    if let Some(salt) = new_kek_salt {
        sqlx::query("UPDATE users SET kek_salt = $1 WHERE id = $2")
            .bind(&salt[..])
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}

// ── Low-level crypto helpers ──────────────────────────────────────────────────

pub fn generate_token(user: &User, secret: &str, expiry_hours: i64) -> AppResult<String> {
    let now = Utc::now();
    let claims = Claims {
        sub: user.id.to_string(),
        email: user.email.clone(),
        exp: (now + Duration::hours(expiry_hours)).timestamp(),
        iat: now.timestamp(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(anyhow::anyhow!("JWT encode error: {}", e)))
}

pub fn verify_token(token: &str, secret: &str) -> AppResult<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|_| AppError::Unauthorized("Invalid or expired token".into()))
}

fn hash_password(password: &str) -> AppResult<String> {
    use argon2::{
        password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
        Argon2,
    };
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Password hash error: {}", e)))
}

fn verify_password(password: &str, hash: &str) -> AppResult<bool> {
    use argon2::{
        password_hash::{PasswordHash, PasswordVerifier},
        Argon2,
    };
    let parsed = PasswordHash::new(hash)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Hash parse error: {}", e)))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

// ── Auth middleware ───────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct AuthUser {
    pub id: Uuid,
    #[allow(dead_code)]
    pub email: String,
}

pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::Unauthorized("Missing authorization header".into()))?;

    let claims = verify_token(token, &state.config.jwt_secret)?;
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Unauthorized("Invalid token subject".into()))?;

    req.extensions_mut().insert(AuthUser {
        id: user_id,
        email: claims.email,
    });

    Ok(next.run(req).await)
}
