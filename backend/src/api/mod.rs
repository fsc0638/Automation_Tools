use axum::{middleware, Router};
use sqlx::PgPool;
use std::sync::Arc;
use crate::config::Config;
use crate::crypto::TokenCipher;

pub mod auth;
pub mod conversation_memory;
pub mod conversations;
pub mod git_identities;
pub mod metrics;
pub mod project_index;
pub mod projects;
pub mod tasks;
pub mod ws;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub cipher: Arc<TokenCipher>,
}

pub fn router(state: AppState) -> Router {
    let public = Router::new()
        .merge(auth::public_routes())
        .merge(ws::routes())  // WS handles its own token auth via query param
        .with_state(state.clone());

    let protected = Router::new()
        .merge(projects::routes())
        .merge(git_identities::routes())
        .merge(conversations::routes())
        .merge(metrics::routes())
        .merge(tasks::routes())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ))
        .with_state(state.clone());

    Router::new()
        .nest("/api", public.merge(protected))
}
