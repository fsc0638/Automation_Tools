use axum::{
    extract::{Path, State},
    response::Json,
    routing::get,
    Extension, Router,
};
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    db::models::{Organization, Workspace},
    error::{AppError, AppResult},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/organizations", get(list_organizations))
        .route(
            "/organizations/:organization_id/workspaces",
            get(list_workspaces),
        )
}

async fn list_organizations(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> AppResult<Json<Vec<Organization>>> {
    let rows: Vec<Organization> = sqlx::query_as(
        "SELECT o.*, om.role
         FROM organizations o
         JOIN organization_members om ON om.organization_id = o.id
         WHERE om.user_id = $1
         ORDER BY o.updated_at DESC",
    )
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn list_workspaces(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(organization_id): Path<Uuid>,
) -> AppResult<Json<Vec<Workspace>>> {
    let member: Option<(Uuid,)> = sqlx::query_as(
        "SELECT organization_id FROM organization_members WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(auth_user.id)
    .fetch_optional(&state.db)
    .await?;
    if member.is_none() {
        return Err(AppError::NotFound("Organization not found".into()));
    }

    let rows: Vec<Workspace> = sqlx::query_as(
        "SELECT w.*, wm.role
         FROM workspaces w
         LEFT JOIN workspace_members wm ON wm.workspace_id = w.id AND wm.user_id = $2
         WHERE w.organization_id = $1
         ORDER BY w.updated_at DESC",
    )
    .bind(organization_id)
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
