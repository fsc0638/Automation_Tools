use axum::{
    extract::{Request, State},
    http::header::AUTHORIZATION,
    middleware::Next,
    response::Response,
    routing::post,
    Json, Router,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    api::AppState,
    db::models::User,
    error::{AppError, AppResult},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user id
    pub email: String,
    pub exp: i64,
    pub iat: i64,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
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

pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
}

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
async fn issue_refresh_token(
    state: &AppState,
    user_id: Uuid,
) -> AppResult<(String, i64)> {
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
    let user_id = row.ok_or_else(|| AppError::Unauthorized("Invalid or expired refresh token".into()))?.0;

    let user: User = sqlx::query_as("SELECT * FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::Unauthorized("User no longer exists".into()))?;

    let access = generate_token(&user, &state.config.jwt_secret, state.config.jwt_expiry_hours)?;
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
    let _ = sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = $1")
        .bind(&hash)
        .execute(&state.db)
        .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> AppResult<Json<AuthResponse>> {
    if req.email.is_empty() || req.password.len() < 8 {
        return Err(AppError::BadRequest(
            "Email required and password must be at least 8 characters".into(),
        ));
    }

    let existing: Option<User> = sqlx::query_as(
        "SELECT * FROM users WHERE email = $1"
    )
    .bind(&req.email)
    .fetch_optional(&state.db)
    .await?;

    if existing.is_some() {
        return Err(AppError::BadRequest("Email already registered".into()));
    }

    let password_hash = hash_password(&req.password)?;
    let user: User = sqlx::query_as(
        "INSERT INTO users (email, password_hash, display_name)
         VALUES ($1, $2, $3)
         RETURNING *",
    )
    .bind(&req.email)
    .bind(&password_hash)
    .bind(&req.display_name)
    .fetch_one(&state.db)
    .await?;

    let access = generate_token(&user, &state.config.jwt_secret, state.config.jwt_expiry_hours)?;
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
    let user: Option<User> = sqlx::query_as("SELECT * FROM users WHERE email = $1")
        .bind(&req.email)
        .fetch_optional(&state.db)
        .await?;

    let user = user.ok_or_else(|| AppError::Unauthorized("Invalid credentials".into()))?;

    if !verify_password(&req.password, &user.password_hash)? {
        return Err(AppError::Unauthorized("Invalid credentials".into()));
    }

    let access = generate_token(&user, &state.config.jwt_secret, state.config.jwt_expiry_hours)?;
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
    use argon2::{password_hash::{rand_core::OsRng, PasswordHasher, SaltString}, Argon2};
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Password hash error: {}", e)))
}

fn verify_password(password: &str, hash: &str) -> AppResult<bool> {
    use argon2::{password_hash::{PasswordHash, PasswordVerifier}, Argon2};
    let parsed = PasswordHash::new(hash)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Hash parse error: {}", e)))?;
    Ok(Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
}

// Middleware
#[derive(Clone, Debug)]
pub struct AuthUser {
    pub id: Uuid,
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
