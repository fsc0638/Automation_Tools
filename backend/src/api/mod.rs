use axum::{extract::State, http::StatusCode, middleware, response::Json, routing::get, Router};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::Arc;
use crate::config::Config;
use crate::crypto::TokenCipher;

pub mod auth;
pub mod agent_profiles;
pub mod conversation_memory;
pub mod conversations;
pub mod epics;
pub mod feedback;
pub mod git_identities;
pub mod metrics;
pub mod project_index;
pub mod projects;
pub mod shared_memory;
pub mod sprints;
pub mod tasks;
pub mod user_views;
pub mod ws;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub cipher: Arc<TokenCipher>,
}

/// Liveness probe. Returns 200 if the process is up; no I/O, no DB.
/// k8s / docker should use this for the readiness gate that just asks
/// "is the binary listening?".
async fn healthz() -> &'static str {
    "ok"
}

/// Readiness probe. Pings the DB with a trivial query. Returns 200 with
/// `{ "db": "ok" }` when reachable, 503 with the error otherwise. Wire
/// this into the orchestrator's actual readiness gate — `/healthz` only
/// confirms the binary is listening, not that downstream deps are up.
async fn readyz(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    match sqlx::query_scalar::<_, i64>("SELECT 1").fetch_one(&state.db).await {
        Ok(_) => (StatusCode::OK, Json(json!({ "db": "ok" }))),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "db": "error", "detail": e.to_string() })),
        ),
    }
}

pub fn router(state: AppState) -> Router {
    let public = Router::new()
        .merge(auth::public_routes())
        .merge(ws::routes())  // WS handles its own token auth via query param
        .with_state(state.clone());

    let protected = Router::new()
        .merge(projects::routes())
        .merge(agent_profiles::routes())
        .merge(git_identities::routes())
        .merge(conversations::routes())
        .merge(metrics::routes())
        .merge(tasks::routes())
        .merge(sprints::routes())
        .merge(epics::routes())
        .merge(shared_memory::routes())
        .merge(user_views::routes())
        .merge(feedback::routes())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ))
        .with_state(state.clone());

    Router::new()
        .nest("/api", public.merge(protected))
        // k8s/docker probes live at the root, not under /api, so they
        // can be hit before the API mount changes.
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .with_state(state)
}
