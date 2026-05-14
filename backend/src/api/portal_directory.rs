//! Read-only endpoints over the portal-imported employee directory.
//!
//! Backs the "attendees" autocomplete on the new-meeting form: pick a
//! department on the left, type a fragment on the right, and the UI shows
//! employees whose Chinese name, employee number, or email local-part
//! matches. The directory itself is refreshed weekly by the Monday 08:00
//! scrape (see portal_sync::run_directory); these handlers just project
//! the resulting rows.

use axum::{
    extract::{Query, State},
    response::Json,
    routing::get,
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{
    api::{auth::AuthUser, AppState},
    error::AppResult,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/portal/departments", get(list_departments))
        .route("/portal/employees/search", get(search_employees))
}

#[derive(Debug, Serialize, FromRow)]
pub struct DepartmentRow {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct EmployeeRow {
    pub employee_no: String,
    pub name: String,
    pub email: Option<String>,
    pub title: Option<String>,
    pub dept_code: Option<String>,
    pub dept_name: Option<String>,
    pub extensions: Vec<String>,
}

async fn list_departments(
    State(state): State<AppState>,
    Extension(_auth_user): Extension<AuthUser>,
) -> AppResult<Json<Vec<DepartmentRow>>> {
    let rows: Vec<DepartmentRow> =
        sqlx::query_as("SELECT code, name FROM portal_departments ORDER BY code")
            .fetch_all(&state.db)
            .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct EmployeeSearchQuery {
    /// Free-text fragment matched against name, employee_no, or the
    /// local-part of the email (the bit before "@"). Empty = no filter.
    #[serde(default)]
    pub q: String,
    /// Optional dept_code filter. Empty / absent = all departments.
    #[serde(default)]
    pub dept_code: String,
    /// Cap rows. Defaults to 20 so the dropdown stays light; clamped to
    /// 100 so a runaway client can't drag the full table back.
    #[serde(default)]
    pub limit: Option<i64>,
}

async fn search_employees(
    State(state): State<AppState>,
    Extension(_auth_user): Extension<AuthUser>,
    Query(q): Query<EmployeeSearchQuery>,
) -> AppResult<Json<Vec<EmployeeRow>>> {
    let limit = q.limit.unwrap_or(20).clamp(1, 100);
    let fragment = q.q.trim().to_string();
    let dept = q.dept_code.trim().to_string();

    let rows: Vec<EmployeeRow> = sqlx::query_as(
        "SELECT pe.employee_no,
                pe.name,
                pe.email,
                pe.title,
                pe.dept_code,
                pd.name AS dept_name,
                pe.extensions
         FROM portal_employees pe
         LEFT JOIN portal_departments pd ON pd.code = pe.dept_code
         WHERE ($1 = ''
                OR LOWER(pe.name) LIKE '%' || LOWER($1) || '%'
                OR pe.employee_no ILIKE '%' || $1 || '%'
                OR LOWER(SPLIT_PART(COALESCE(pe.email, ''), '@', 1)) LIKE '%' || LOWER($1) || '%')
           AND ($2 = '' OR pe.dept_code = $2)
         ORDER BY pe.name
         LIMIT $3",
    )
    .bind(&fragment)
    .bind(&dept)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}
