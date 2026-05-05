use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod agents;
mod api;
mod config;
mod db;
mod error;
mod git_ops;

use api::{router, AppState};

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

    let state = AppState {
        db,
        config: config.clone(),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = router(state).layer(TraceLayer::new_for_http()).layer(cors);

    let addr = format!("{}:{}", config.server_host, config.server_port);
    tracing::info!("Kway Dev Backend listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
