use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, patch as http_patch},
    Extension, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
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
        .route(
            "/organizations/:organization_id/members",
            get(list_organization_members).post(add_organization_member),
        )
        .route(
            "/organizations/:organization_id/members/:user_id",
            http_patch(update_organization_member).delete(remove_organization_member),
        )
        .route("/projects/:project_id/acl", get(list_project_acl).post(add_project_acl))
        .route(
            "/projects/:project_id/acl/:user_id",
            http_patch(update_project_acl).delete(remove_project_acl),
        )
}

#[derive(Debug, Serialize, FromRow)]
pub struct MemberRow {
    pub user_id: Uuid,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct MemberInput {
    pub email: Option<String>,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct RoleInput {
    pub role: String,
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
    require_org_member(&state, organization_id, auth_user.id).await?;

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

async fn list_organization_members(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(organization_id): Path<Uuid>,
) -> AppResult<Json<Vec<MemberRow>>> {
    require_org_member(&state, organization_id, auth_user.id).await?;
    let rows = sqlx::query_as(
        "SELECT om.user_id, u.email, u.display_name, om.role, om.created_at
         FROM organization_members om
         JOIN users u ON u.id = om.user_id
         WHERE om.organization_id = $1
         ORDER BY access_role_rank(om.role) DESC, u.display_name ASC",
    )
    .bind(organization_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn add_organization_member(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(organization_id): Path<Uuid>,
    Json(req): Json<MemberInput>,
) -> AppResult<(StatusCode, Json<MemberRow>)> {
    require_org_admin(&state, organization_id, auth_user.id).await?;
    validate_org_role(&req.role)?;
    let email = req.email.as_deref().unwrap_or_default().trim().to_lowercase();
    if email.is_empty() {
        return Err(AppError::BadRequest("Email is required".into()));
    }
    let user_id = find_user_by_email(&state, &email).await?;

    sqlx::query(
        "INSERT INTO organization_members (organization_id, user_id, role)
         VALUES ($1, $2, $3)
         ON CONFLICT (organization_id, user_id) DO UPDATE SET role = EXCLUDED.role",
    )
    .bind(organization_id)
    .bind(user_id)
    .bind(&req.role)
    .execute(&state.db)
    .await?;

    let row = get_org_member_row(&state, organization_id, user_id).await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn update_organization_member(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((organization_id, user_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<RoleInput>,
) -> AppResult<Json<MemberRow>> {
    require_org_admin(&state, organization_id, auth_user.id).await?;
    validate_org_role(&req.role)?;
    protect_last_org_owner(&state, organization_id, user_id, Some(&req.role)).await?;

    let result = sqlx::query(
        "UPDATE organization_members SET role = $1 WHERE organization_id = $2 AND user_id = $3",
    )
    .bind(&req.role)
    .bind(organization_id)
    .bind(user_id)
    .execute(&state.db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Organization member not found".into()));
    }

    Ok(Json(get_org_member_row(&state, organization_id, user_id).await?))
}

async fn remove_organization_member(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((organization_id, user_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    require_org_admin(&state, organization_id, auth_user.id).await?;
    protect_last_org_owner(&state, organization_id, user_id, None).await?;

    let result = sqlx::query(
        "DELETE FROM organization_members WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .execute(&state.db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Organization member not found".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn list_project_acl(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
) -> AppResult<Json<Vec<MemberRow>>> {
    require_project_access(&state, project_id, auth_user.id, "viewer").await?;
    let rows = sqlx::query_as(
        "SELECT pa.user_id, u.email, u.display_name, pa.role, pa.created_at
         FROM project_acl pa
         JOIN users u ON u.id = pa.user_id
         WHERE pa.project_id = $1
         ORDER BY access_role_rank(pa.role) DESC, u.display_name ASC",
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn add_project_acl(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
    Json(req): Json<MemberInput>,
) -> AppResult<(StatusCode, Json<MemberRow>)> {
    require_project_access(&state, project_id, auth_user.id, "admin").await?;
    validate_project_role(&req.role)?;
    let email = req.email.as_deref().unwrap_or_default().trim().to_lowercase();
    if email.is_empty() {
        return Err(AppError::BadRequest("Email is required".into()));
    }
    let user_id = find_user_by_email(&state, &email).await?;

    sqlx::query(
        "INSERT INTO project_acl (project_id, user_id, role)
         VALUES ($1, $2, $3)
         ON CONFLICT (project_id, user_id) DO UPDATE SET role = EXCLUDED.role",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(&req.role)
    .execute(&state.db)
    .await?;

    let row = get_project_acl_row(&state, project_id, user_id).await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn update_project_acl(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, user_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<RoleInput>,
) -> AppResult<Json<MemberRow>> {
    require_project_access(&state, project_id, auth_user.id, "admin").await?;
    validate_project_role(&req.role)?;
    protect_last_project_owner(&state, project_id, user_id, Some(&req.role)).await?;

    let result = sqlx::query("UPDATE project_acl SET role = $1 WHERE project_id = $2 AND user_id = $3")
        .bind(&req.role)
        .bind(project_id)
        .bind(user_id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Project member not found".into()));
    }

    Ok(Json(get_project_acl_row(&state, project_id, user_id).await?))
}

async fn remove_project_acl(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, user_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    require_project_access(&state, project_id, auth_user.id, "admin").await?;
    protect_last_project_owner(&state, project_id, user_id, None).await?;

    let result = sqlx::query("DELETE FROM project_acl WHERE project_id = $1 AND user_id = $2")
        .bind(project_id)
        .bind(user_id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Project member not found".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn require_org_member(state: &AppState, organization_id: Uuid, user_id: Uuid) -> AppResult<()> {
    let exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT organization_id FROM organization_members WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    exists
        .map(|_| ())
        .ok_or_else(|| AppError::NotFound("Organization not found".into()))
}

async fn require_org_admin(state: &AppState, organization_id: Uuid, user_id: Uuid) -> AppResult<()> {
    let allowed: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM organization_members
            WHERE organization_id = $1 AND user_id = $2
              AND access_role_rank(role) >= access_role_rank('admin')
        )",
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;
    if allowed {
        Ok(())
    } else {
        Err(AppError::NotFound("Organization not found".into()))
    }
}

async fn require_project_access(
    state: &AppState,
    project_id: Uuid,
    user_id: Uuid,
    min_role: &str,
) -> AppResult<()> {
    let allowed: bool = sqlx::query_scalar("SELECT user_can_access_project($1, $2, $3)")
        .bind(project_id)
        .bind(user_id)
        .bind(min_role)
        .fetch_one(&state.db)
        .await?;
    if allowed {
        Ok(())
    } else {
        Err(AppError::NotFound("Project not found".into()))
    }
}

async fn find_user_by_email(state: &AppState, email: &str) -> AppResult<Uuid> {
    let user_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE lower(email) = $1")
        .bind(email)
        .fetch_optional(&state.db)
        .await?;
    user_id.ok_or_else(|| AppError::NotFound("User not found".into()))
}

async fn get_org_member_row(state: &AppState, organization_id: Uuid, user_id: Uuid) -> AppResult<MemberRow> {
    sqlx::query_as(
        "SELECT om.user_id, u.email, u.display_name, om.role, om.created_at
         FROM organization_members om
         JOIN users u ON u.id = om.user_id
         WHERE om.organization_id = $1 AND om.user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map_err(Into::into)
}

async fn get_project_acl_row(state: &AppState, project_id: Uuid, user_id: Uuid) -> AppResult<MemberRow> {
    sqlx::query_as(
        "SELECT pa.user_id, u.email, u.display_name, pa.role, pa.created_at
         FROM project_acl pa
         JOIN users u ON u.id = pa.user_id
         WHERE pa.project_id = $1 AND pa.user_id = $2",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map_err(Into::into)
}

async fn protect_last_org_owner(
    state: &AppState,
    organization_id: Uuid,
    user_id: Uuid,
    next_role: Option<&str>,
) -> AppResult<()> {
    if next_role == Some("owner") {
        return Ok(());
    }
    let current_role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM organization_members WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    if current_role.as_deref() != Some("owner") {
        return Ok(());
    }
    let owner_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM organization_members WHERE organization_id = $1 AND role = 'owner'",
    )
    .bind(organization_id)
    .fetch_one(&state.db)
    .await?;
    if owner_count <= 1 {
        return Err(AppError::BadRequest("At least one organization owner is required".into()));
    }
    Ok(())
}

async fn protect_last_project_owner(
    state: &AppState,
    project_id: Uuid,
    user_id: Uuid,
    next_role: Option<&str>,
) -> AppResult<()> {
    if next_role == Some("owner") {
        return Ok(());
    }
    let current_role: Option<String> =
        sqlx::query_scalar("SELECT role FROM project_acl WHERE project_id = $1 AND user_id = $2")
            .bind(project_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    if current_role.as_deref() != Some("owner") {
        return Ok(());
    }
    let owner_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_acl WHERE project_id = $1 AND role = 'owner'")
            .bind(project_id)
            .fetch_one(&state.db)
            .await?;
    if owner_count <= 1 {
        return Err(AppError::BadRequest("At least one project owner is required".into()));
    }
    Ok(())
}

fn validate_org_role(role: &str) -> AppResult<()> {
    if matches!(role, "owner" | "admin" | "member" | "viewer") {
        Ok(())
    } else {
        Err(AppError::BadRequest("Invalid organization role".into()))
    }
}

fn validate_project_role(role: &str) -> AppResult<()> {
    if matches!(role, "owner" | "admin" | "editor" | "viewer") {
        Ok(())
    } else {
        Err(AppError::BadRequest("Invalid project role".into()))
    }
}
