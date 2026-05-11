use axum::{
    body::Body,
    http::{HeaderValue, Request, Response},
    middleware::{self as axum_middleware, Next},
    routing::get,
    Router,
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::Arc;
use std::time::Instant;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

mod agents;
mod api;
mod config;
mod crypto;
mod db;
mod error;
mod git_ops;
mod security;

use api::{router, AppState};
use crypto::TokenCipher;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    // Log format gating: LOG_FORMAT=json uses single-line JSON suitable
    // for ingestion (Loki/ELK/Datadog/CloudWatch). Anything else stays
    // on the pretty human-readable formatter for local dev.
    let log_format = std::env::var("LOG_FORMAT").unwrap_or_else(|_| "pretty".into());
    let env_filter = tracing_subscriber::EnvFilter::new(
        std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
    );
    if log_format.eq_ignore_ascii_case("json") {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_current_span(true)
                    .with_span_list(false),
            )
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
    }

    let config = Arc::new(config::Config::from_env()?);
    let db = db::create_pool(&config.database_url).await?;
    db::run_migrations(&db).await?;

    // Prometheus exporter — installs the global metrics recorder and
    // hands back a handle we use to render the /metrics endpoint.
    let prom_handle: PrometheusHandle = PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install prometheus recorder");

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
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any)
        }
    };

    // Per-request tracing span with a trace_id (UUID v4). Honors an
    // inbound `X-Request-Id` header when present so upstream proxies /
    // load-balancers can stitch the same id through multiple hops; falls
    // back to a fresh UUID otherwise. The id is attached as a span field
    // so all logs emitted while handling the request are tagged with it.
    let trace_layer = TraceLayer::new_for_http().make_span_with(|req: &axum::http::Request<_>| {
        let trace_id = req
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        tracing::info_span!(
            "http_request",
            trace_id = %trace_id,
            method = %req.method(),
            uri = %req.uri(),
            version = ?req.version(),
        )
    });

    // Per-request metrics middleware. Counts requests by method+status
    // and observes response duration. Excludes /metrics itself to avoid
    // an observer self-loop dominating its own histogram.
    async fn metrics_mw(req: Request<Body>, next: Next) -> Response<Body> {
        let path = req.uri().path().to_string();
        if path == "/metrics" {
            return next.run(req).await;
        }
        let method = req.method().to_string();
        let start = Instant::now();
        let response = next.run(req).await;
        let status = response.status().as_u16().to_string();
        let elapsed = start.elapsed().as_secs_f64();
        metrics::counter!(
            "http_requests_total",
            "method" => method.clone(),
            "status" => status.clone(),
        )
        .increment(1);
        metrics::histogram!(
            "http_request_duration_seconds",
            "method" => method,
            "status" => status,
        )
        .record(elapsed);
        response
    }

    let metrics_router: Router = Router::new().route(
        "/metrics",
        get(move || {
            let handle = prom_handle.clone();
            async move { handle.render() }
        }),
    );

    let app = router(state)
        .merge(metrics_router)
        .layer(axum_middleware::from_fn(metrics_mw))
        .layer(trace_layer)
        .layer(cors);

    let addr = format!("{}:{}", config.server_host, config.server_port);
    tracing::info!("Kway Dev Backend listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
