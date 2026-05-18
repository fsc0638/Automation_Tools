use axum::{
    body::Body,
    http::{HeaderValue, Request, Response},
    middleware::{self as axum_middleware, Next},
    routing::get,
    Router,
};
use chrono::{Local, Timelike};
use kway_dev_backend::portal_sync::{self, SyncLock, SyncOptions};
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
mod grounding;
mod security;

use api::{router, AppState};
use crypto::TokenCipher;
use sqlx::PgPool;

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

    let portal_sync_lock = SyncLock::new();
    // AgentK-aligned: broadcast::channel for meeting lifecycle events.
    // Capacity 256 absorbs short bursts (e.g. status flip + portal book
    // + invitation send in quick succession). Lagging subscribers get
    // RecvError::Lagged on the WS side and refetch.
    let (meeting_events_tx, _) = tokio::sync::broadcast::channel(256);

    let state = AppState {
        db: db.clone(),
        config: config.clone(),
        cipher,
        portal_sync_lock: portal_sync_lock.clone(),
        meeting_events: meeting_events_tx,
    };

    // Background portal-sync scheduler. Wakes every 30 mins aligned to
    // :00 / :30 and, if the local hour is in the working window, fires
    // a scrape + import. Shares the same lock the /meetings/sync endpoint
    // uses, so a manual click can't overlap an in-flight auto sync.
    if std::env::var("PORTAL_SYNC_ENABLED")
        .ok()
        .map(|v| !matches!(v.to_lowercase().as_str(), "0" | "false" | "no"))
        .unwrap_or(true)
    {
        tokio::spawn(portal_sync_scheduler_loop(
            db.clone(),
            portal_sync_lock.clone(),
        ));
    } else {
        tracing::info!("portal_sync scheduler disabled via PORTAL_SYNC_ENABLED=0");
    }

    // Weekly employee-directory scraper. Fires Monday 08:00 local time;
    // shares the same SyncLock as the meeting-rooms scheduler so both
    // can't drive the Playwright session at once. Result + errors are
    // appended to kway_portal/output/employee-directory/sync.log.
    if std::env::var("PORTAL_DIRECTORY_SYNC_ENABLED")
        .ok()
        .map(|v| !matches!(v.to_lowercase().as_str(), "0" | "false" | "no"))
        .unwrap_or(true)
    {
        tokio::spawn(portal_directory_weekly_loop(db.clone(), portal_sync_lock));
    } else {
        tracing::info!(
            "portal_directory scheduler disabled via PORTAL_DIRECTORY_SYNC_ENABLED=0"
        );
    }

    // AgentK-aligned: meeting_files retention sweep. Wakes once an hour,
    // finds soft-deleted files past their hard_delete_after, removes the
    // on-disk file + DB row. Lazy by design — we don't need second-level
    // accuracy and an hourly tick keeps it out of the critical path.
    tokio::spawn(meeting_files_retention_sweep_loop(db.clone()));

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

/// Periodic portal sync. Sleeps until the next :00 or :30 tick, then runs
/// a sync iff the local hour is in the working window (env
/// PORTAL_SYNC_HOURS, default "08-21"; lower bound inclusive, upper bound
/// exclusive). Errors are logged and the loop keeps running.
async fn portal_sync_scheduler_loop(db: PgPool, lock: SyncLock) {
    let (start_hour, end_hour) = parse_work_hours(
        std::env::var("PORTAL_SYNC_HOURS").as_deref().unwrap_or("08-21"),
    )
    .unwrap_or((8, 21));
    let backend_cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    tracing::info!(
        "portal_sync scheduler: every 30 min during {start_hour:02}:00–{end_hour:02}:00"
    );

    loop {
        let now = Local::now();
        let next = next_half_hour(now);
        let wait = (next - now)
            .to_std()
            .unwrap_or_else(|_| std::time::Duration::from_secs(60));
        tokio::time::sleep(wait).await;

        let hour = Local::now().hour();
        if hour < start_hour || hour >= end_hour {
            continue;
        }

        let opts = SyncOptions::from_env(&backend_cwd);
        match portal_sync::run(&db, &lock, &opts).await {
            Ok(report) => {
                tracing::info!(
                    inserted = report.inserted,
                    updated = report.updated,
                    unchanged = report.unchanged,
                    cancelled = report.cancelled,
                    elapsed_ms = report.elapsed_ms,
                    "portal_sync auto run ok"
                );
            }
            Err(e) => {
                tracing::warn!("portal_sync auto run failed: {e:#}");
            }
        }
    }
}

fn next_half_hour(now: chrono::DateTime<Local>) -> chrono::DateTime<Local> {
    let target_minute = if now.minute() < 30 { 30 } else { 60 };
    let mut next = now
        .with_minute(0)
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap_or(now);
    next += chrono::Duration::minutes(target_minute as i64);
    // chrono allows minute=60 by overflowing into the next hour, but
    // with_minute(60) refuses, so we set 0 + add 60 minutes (done above).
    // Guard against pathological clock skew giving us a past instant.
    if next <= now {
        next = next + chrono::Duration::minutes(30);
    }
    next
}

fn parse_work_hours(spec: &str) -> Option<(u32, u32)> {
    let (a, b) = spec.split_once('-')?;
    let start: u32 = a.trim().parse().ok()?;
    let end: u32 = b.trim().parse().ok()?;
    if start < 24 && end <= 24 && start < end {
        Some((start, end))
    } else {
        None
    }
}

/// Weekly employee-directory sync. Sleeps until next Monday 08:00 local,
/// fires the scrape + import, repeats. Errors logged to tracing + the
/// feature's sync.log (handled inside portal_sync::run_directory).
async fn portal_directory_weekly_loop(db: PgPool, lock: SyncLock) {
    let backend_cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    tracing::info!("portal_directory scheduler: Monday 08:00 local time");

    loop {
        let now = Local::now();
        let next = next_monday_at_08(now);
        let wait = (next - now)
            .to_std()
            .unwrap_or_else(|_| std::time::Duration::from_secs(60));
        tracing::info!(
            "portal_directory: next run at {} ({}s from now)",
            next.format("%Y-%m-%d %H:%M:%S"),
            wait.as_secs()
        );
        tokio::time::sleep(wait).await;

        let opts = SyncOptions::from_env(&backend_cwd);
        match portal_sync::run_directory(&db, &lock, &opts).await {
            Ok(r) => tracing::info!(
                week = r.week_starting,
                departments = r.departments_upserted,
                employees = r.employees_upserted,
                linked_users = r.employees_linked_to_user,
                elapsed_ms = r.elapsed_ms,
                "portal_directory auto run ok"
            ),
            Err(e) => tracing::warn!("portal_directory auto run failed: {e:#}"),
        }
    }
}

/// The next Monday-at-08:00 strictly after `now`. If `now` is Monday and
/// it's not yet 08:00, returns today 08:00. Otherwise jumps to next week.
fn next_monday_at_08(now: chrono::DateTime<Local>) -> chrono::DateTime<Local> {
    use chrono::{Datelike, Weekday};
    // Distance in days to the next Monday (0 if today IS Monday).
    let dow_offset = match now.weekday() {
        Weekday::Mon => 0,
        Weekday::Tue => 6,
        Weekday::Wed => 5,
        Weekday::Thu => 4,
        Weekday::Fri => 3,
        Weekday::Sat => 2,
        Weekday::Sun => 1,
    };
    let candidate = now
        .with_hour(8)
        .and_then(|d| d.with_minute(0))
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap_or(now)
        + chrono::Duration::days(dow_offset);
    // If it's already Monday past 08:00 (or current time >= candidate),
    // bump a full week ahead.
    if candidate <= now {
        candidate + chrono::Duration::days(7)
    } else {
        candidate
    }
}

/// AgentK-aligned soft-delete sweep. Hourly tick: any meeting_files row
/// whose `hard_delete_after` has passed gets its on-disk file removed
/// (best-effort) and the DB row deleted. Errors are logged but never
/// propagate — a stuck file shouldn't take the server down with it.
async fn meeting_files_retention_sweep_loop(db: PgPool) {
    use std::time::Duration as StdDuration;
    tracing::info!("meeting_files retention sweep: hourly tick");
    // Initial 60s delay so first sweep doesn't fight with startup work.
    tokio::time::sleep(StdDuration::from_secs(60)).await;
    loop {
        let now = chrono::Utc::now();
        let rows: Vec<(uuid::Uuid, String)> = match sqlx::query_as(
            "SELECT id, storage_path FROM meeting_files
             WHERE hard_delete_after IS NOT NULL
               AND hard_delete_after <= NOW()",
        )
        .fetch_all(&db)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("retention sweep: query failed: {e:?}");
                tokio::time::sleep(StdDuration::from_secs(3600)).await;
                continue;
            }
        };
        let mut removed = 0usize;
        for (id, path) in &rows {
            let _ = std::fs::remove_file(path);
            if let Err(e) = sqlx::query("DELETE FROM meeting_files WHERE id = $1")
                .bind(id)
                .execute(&db)
                .await
            {
                tracing::warn!("retention sweep: delete row {id} failed: {e:?}");
                continue;
            }
            removed += 1;
        }
        if removed > 0 {
            tracing::info!(
                "retention sweep at {}: purged {removed} files",
                now.format("%Y-%m-%d %H:%M:%S"),
            );
        }
        tokio::time::sleep(StdDuration::from_secs(3600)).await;
    }
}
