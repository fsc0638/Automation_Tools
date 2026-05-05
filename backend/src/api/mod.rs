use axum::{middleware, Router};
use sqlx::PgPool;
use std::sync::Arc;
use crate::config::Config;

pub mod auth;
pub mod conversations;
pub mod projects;
pub mod ws;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
}

pub fn router(state: AppState) -> Router {
    let public = Router::new()
        .merge(auth::public_routes())
        .with_state(state.clone());

    let protected = Router::new()
        .merge(projects::routes())
        .merge(conversations::routes())
        .merge(ws::routes())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ))
        .with_state(state.clone());

    Router::new()
        .nest("/api", public.merge(protected))
}
