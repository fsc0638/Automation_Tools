use axum::http::HeaderValue;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod agents;
mod api;
mod config;
mod crypto;
mod db;
mod error;
mod git_ops;

use api::{router, AppState};
use crypto::TokenCipher;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Arc::new(config::Config::from_env()?);
    let db = db::create_pool(&config.database_url).await?;
    db::run_migrations(&db).await?;

    let cipher = match std::env::var("GIT_TOKEN_ENCRYPTION_KEY") {
        Ok(k) if !k.trim().is_empty() => Arc::new(TokenCipher::from_base64_key(&k)?),
        _ => {
            tracing::warn!(
                "GIT_TOKEN_ENCRYPTION_KEY not set; deriving from JWT_SECRET. \
                Generate a dedicated key with: openssl rand -base64 32"
            );
            Arc::new(TokenCipher::from_passphrase(&config.jwt_secret))
        }
    };

    let state = AppState {
        db,
        config: config.clone(),
        cipher,
    };

    let cors = match std::env::var("CORS_ALLOWED_ORIGINS")
        .ok()
        .filter(|s| !s.trim().is_empty() && s.trim() != "*")
    {
        Some(raw) => {
            let origins: Vec<HeaderValue> = raw
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .filter_map(|s| HeaderValue::from_str(s).ok())
                .collect();
            tracing::info!("CORS locked to {} allowed origin(s)", origins.len());
            CorsLayer::new()
                .allow_origin(AllowOrigin::list(origins))
                .allow_methods(Any)
                .allow_headers(Any)
        }
        None => {
            tracing::warn!(
                "CORS_ALLOWED_ORIGINS not set or '*' — allowing any origin. \
                Set CORS_ALLOWED_ORIGINS=https://your-domain.com in production."
            );
            CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any)
        }
    };

    let app = router(state).layer(TraceLayer::new_for_http()).layer(cors);

    let addr = format!("{}:{}", config.server_host, config.server_port);
    tracing::info!("Kway Dev Backend listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
