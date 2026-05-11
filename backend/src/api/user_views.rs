//! User-scoped cross-project queries: a global Roadmap (B1), global
//! conversation search (B2), global usage / cost aggregation (B4), and
//! a cross-project code search (B5). Everything in this module is
//! filtered through project ACLs so each user only sees projects they can access.

use axum::{
    extract::{Query, State},
    response::Json,
    routing::get,
    Extension, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    error::AppResult,
};

// ---------------------------------------------------------------------
// B1: Global Roadmap — all of the user's tasks, joined with project +
// epic + sprint info so the cross-project board can render chips.
// ---------------------------------------------------------------------

#[derive(Debug, Serialize, FromRow)]
pub struct UserTask {
    pub id: Uuid,
    pub project_id: Uuid,
    pub project_name: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub assignee: Option<String>,
    pub due_date: Option<NaiveDate>,
    pub labels: Vec<String>,
    pub sprint_id: Option<Uuid>,
    pub sprint_name: Option<String>,
    pub epic_id: Option<Uuid>,
    pub epic_name: Option<String>,
    pub linked_pr_url: Option<String>,
    pub comment_count: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UserTaskQuery {
    pub project_id: Option<Uuid>,
    pub epic_id: Option<Uuid>,
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub label: Option<String>,
    pub q: Option<String>,
}

async fn list_user_tasks(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(query): Query<UserTaskQuery>,
) -> AppResult<Json<Vec<UserTask>>> {
    let q_pattern = query
        .q
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", s.to_lowercase()));

    let rows: Vec<UserTask> = sqlx::query_as(
        "SELECT t.id, t.project_id, p.name AS project_name,
                t.title, t.status, t.priority, t.assignee, t.due_date,
                t.labels, t.sprint_id, sp.name AS sprint_name,
                t.epic_id, e.name AS epic_name,
                t.linked_pr_url,
                COALESCE(cc.n, 0)::int8 AS comment_count,
                t.updated_at
         FROM project_tasks t
         JOIN projects   p  ON p.id  = t.project_id AND user_can_access_project(p.id, $1, 'viewer')
         LEFT JOIN sprints sp ON sp.id = t.sprint_id
         LEFT JOIN epics   e  ON e.id  = t.epic_id
         LEFT JOIN (
            SELECT task_id, COUNT(*)::int8 AS n FROM task_comments GROUP BY task_id
         ) cc ON cc.task_id = t.id
         WHERE ($2::uuid    IS NULL OR t.project_id = $2)
           AND ($3::uuid    IS NULL OR t.epic_id    = $3)
           AND ($4::text    IS NULL OR t.status     = $4)
           AND ($5::text    IS NULL OR t.assignee   = $5)
           AND ($6::text    IS NULL OR $6 = ANY(t.labels))
           AND ($7::text    IS NULL OR LOWER(t.title) LIKE $7
                                    OR LOWER(COALESCE(t.why,'')) LIKE $7)
         ORDER BY
            CASE t.priority WHEN 'critical' THEN 0 WHEN 'high' THEN 1
                            WHEN 'medium' THEN 2 ELSE 3 END,
            t.updated_at DESC",
    )
    .bind(auth_user.id)
    .bind(query.project_id)
    .bind(query.epic_id)
    .bind(query.status.as_deref())
    .bind(query.assignee.as_deref())
    .bind(query.label.as_deref())
    .bind(q_pattern)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}

// ---------------------------------------------------------------------
// B4: Global usage / cost across all of the user's projects.
// ---------------------------------------------------------------------

#[derive(Debug, Serialize, FromRow)]
pub struct ProjectUsage {
    pub project_id: Uuid,
    pub project_name: String,
    pub calls: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost_usd: f64,
}

/// Per-agent usage rollup so the dashboard can show OpenClaw vs Hermes vs
/// custom agents side-by-side. `agent` is the raw column from
/// `agent_usage_events` ("openclaw" / "hermes" / custom-agent slug).
#[derive(Debug, Serialize, FromRow)]
pub struct AgentUsage {
    pub agent: String,
    pub calls: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost_usd: f64,
}

#[derive(Debug, Serialize)]
pub struct UserUsage {
    pub by_project: Vec<ProjectUsage>,
    /// New in DEFERRED 11: cost / calls / tokens broken down by agent role.
    pub by_agent: Vec<AgentUsage>,
    pub total_calls: i64,
    pub total_tokens_in: i64,
    pub total_tokens_out: i64,
    pub total_cost_usd: f64,
    /// Daily aggregate across all projects so the dashboard can render
    /// a single time-series for the user.
    pub daily: Vec<JsonValue>,
    /// Window size in days actually used (echoes the `days` query param
    /// after clamping). Lets the UI render "Last N days" labels safely.
    pub days: i64,
}

#[derive(Debug, Deserialize, Default)]
pub struct UsageQuery {
    /// Lookback window in days. Default 30, clamped to [1, 365].
    #[serde(default)]
    pub days: Option<i64>,
}

fn clamp_days(raw: Option<i64>) -> i64 {
    raw.unwrap_or(30).clamp(1, 365)
}

async fn user_usage(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(q): Query<UsageQuery>,
) -> AppResult<Json<UserUsage>> {
    let days = clamp_days(q.days);

    // NOTE: We splice `days` directly into the SQL as `INTERVAL '<n> days'`.
    // It's a clamped integer, never a user-controlled string, so this is
    // safe from injection. Parameterising INTERVAL via sqlx is awkward
    // because Postgres expects either a literal or `make_interval(...)`,
    // and we already validate the value above.
    let by_project_sql = format!(
        "SELECT
            p.id   AS project_id,
            p.name AS project_name,
            COUNT(e.*)::int8                                AS calls,
            COALESCE(SUM(e.input_tokens),  0)::int8         AS tokens_in,
            COALESCE(SUM(e.output_tokens), 0)::int8         AS tokens_out,
            COALESCE(SUM(e.cost_usd),      0)::float8       AS cost_usd
         FROM projects p
         LEFT JOIN agent_usage_events e ON e.project_id = p.id
            AND e.created_at >= NOW() - INTERVAL '{days} days'
         WHERE user_can_access_project(p.id, $1, 'viewer')
         GROUP BY p.id, p.name
         ORDER BY cost_usd DESC, calls DESC"
    );
    let by_project: Vec<ProjectUsage> = sqlx::query_as(&by_project_sql)
        .bind(auth_user.id)
        .fetch_all(&state.db)
        .await?;

    let by_agent_sql = format!(
        "SELECT
            e.agent                                         AS agent,
            COUNT(*)::int8                                  AS calls,
            COALESCE(SUM(e.input_tokens),  0)::int8         AS tokens_in,
            COALESCE(SUM(e.output_tokens), 0)::int8         AS tokens_out,
            COALESCE(SUM(e.cost_usd),      0)::float8       AS cost_usd
         FROM agent_usage_events e
         JOIN projects p ON p.id = e.project_id AND user_can_access_project(p.id, $1, 'viewer')
         WHERE e.created_at >= NOW() - INTERVAL '{days} days'
         GROUP BY e.agent
         ORDER BY cost_usd DESC, calls DESC"
    );
    let by_agent: Vec<AgentUsage> = sqlx::query_as(&by_agent_sql)
        .bind(auth_user.id)
        .fetch_all(&state.db)
        .await?;

    let total_calls       = by_project.iter().map(|r| r.calls).sum();
    let total_tokens_in   = by_project.iter().map(|r| r.tokens_in).sum();
    let total_tokens_out  = by_project.iter().map(|r| r.tokens_out).sum();
    let total_cost_usd    = by_project.iter().map(|r| r.cost_usd).sum();

    let daily_sql = format!(
        "SELECT to_jsonb(d) FROM (
            SELECT
                DATE_TRUNC('day', e.created_at)::date AS day,
                COUNT(*)::int8                          AS calls,
                COALESCE(SUM(e.cost_usd), 0)::float8    AS cost_usd
            FROM agent_usage_events e
            JOIN projects p ON p.id = e.project_id AND user_can_access_project(p.id, $1, 'viewer')
            WHERE e.created_at >= NOW() - INTERVAL '{days} days'
            GROUP BY 1
            ORDER BY 1
         ) d"
    );
    let daily: Vec<JsonValue> = sqlx::query_scalar(&daily_sql)
        .bind(auth_user.id)
        .fetch_all(&state.db)
        .await?;

    Ok(Json(UserUsage {
        by_project,
        by_agent,
        total_calls,
        total_tokens_in,
        total_tokens_out,
        total_cost_usd,
        daily,
        days,
    }))
}

// ---------------------------------------------------------------------
// DEFERRED 14 + 15: Cross-project debate health rollup.
//
// metrics_summary inside `api/metrics.rs` exposes consensus rate, round
// distribution, and file-citation rate, but only one project at a time.
// This endpoint aggregates the same primitives across every project the
// user has access to so the global Insights page can show a single
// platform-wide picture.
// ---------------------------------------------------------------------

#[derive(Debug, Serialize, FromRow)]
pub struct ProjectDebateHealth {
    pub project_id: Uuid,
    pub project_name: String,
    pub debate_turns: i64,
    pub consensus_turns: i64,
    pub citation_turns: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct RoundBucket {
    pub rounds: i32,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct DebateHealth {
    pub days: i64,
    pub total_debate_turns: i64,
    pub consensus_rate: f64,
    pub file_citation_rate: f64,
    pub by_project: Vec<ProjectDebateHealth>,
    pub round_distribution: Vec<RoundBucket>,
}

async fn user_debate_health(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(q): Query<UsageQuery>,
) -> AppResult<Json<DebateHealth>> {
    let days = clamp_days(q.days);

    // Per-project rollup: count debate turns, how many had a consensus
    // marker, and how many cited at least one file.
    let by_project_sql = format!(
        "SELECT
            p.id                                                  AS project_id,
            p.name                                                AS project_name,
            COUNT(*) FILTER (WHERE e.mode = 'debate')::int8       AS debate_turns,
            COUNT(*) FILTER (
                WHERE e.mode = 'debate' AND e.has_consensus_marker
            )::int8                                                AS consensus_turns,
            COUNT(*) FILTER (
                WHERE e.has_file_citation
            )::int8                                                AS citation_turns
         FROM projects p
         LEFT JOIN agent_usage_events e ON e.project_id = p.id
            AND e.created_at >= NOW() - INTERVAL '{days} days'
         WHERE user_can_access_project(p.id, $1, 'viewer')
         GROUP BY p.id, p.name
         ORDER BY debate_turns DESC, p.name"
    );
    let by_project: Vec<ProjectDebateHealth> = sqlx::query_as(&by_project_sql)
        .bind(auth_user.id)
        .fetch_all(&state.db)
        .await?;

    // Round distribution: for debate mode, count how many turns settled
    // at each round number. Useful for spotting "always-9-rounds" patterns
    // that suggest the consensus heuristic is too strict.
    let round_sql = format!(
        "SELECT
            COALESCE(e.round_number, 0)::int4 AS rounds,
            COUNT(*)::int8                    AS count
         FROM agent_usage_events e
         JOIN projects p ON p.id = e.project_id AND user_can_access_project(p.id, $1, 'viewer')
         WHERE e.mode = 'debate'
           AND e.phase = 'final'
           AND e.created_at >= NOW() - INTERVAL '{days} days'
         GROUP BY 1
         ORDER BY 1"
    );
    let round_distribution: Vec<RoundBucket> = sqlx::query_as(&round_sql)
        .bind(auth_user.id)
        .fetch_all(&state.db)
        .await?;

    let total_debate_turns: i64 = by_project.iter().map(|r| r.debate_turns).sum();
    let consensus_turns:    i64 = by_project.iter().map(|r| r.consensus_turns).sum();
    let citation_eligible:  i64 = by_project.iter().map(|r| r.debate_turns).sum();
    let citation_turns:     i64 = by_project.iter().map(|r| r.citation_turns).sum();

    let consensus_rate = if total_debate_turns > 0 {
        consensus_turns as f64 / total_debate_turns as f64
    } else {
        0.0
    };
    let file_citation_rate = if citation_eligible > 0 {
        citation_turns as f64 / citation_eligible as f64
    } else {
        0.0
    };

    Ok(Json(DebateHealth {
        days,
        total_debate_turns,
        consensus_rate,
        file_citation_rate,
        by_project,
        round_distribution,
    }))
}

// ---------------------------------------------------------------------
// B2: Global conversation search.
// ---------------------------------------------------------------------

#[derive(Debug, Serialize, FromRow)]
pub struct ConvHit {
    pub conversation_id: Uuid,
    pub project_id: Uuid,
    pub project_name: String,
    pub title: String,
    pub mode: String,
    pub message_id: Option<Uuid>,
    pub snippet: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default)]
    pub limit: Option<i64>,
}

async fn search_conversations(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(req): Query<SearchQuery>,
) -> AppResult<Json<Vec<ConvHit>>> {
    let q = req.q.trim();
    if q.is_empty() {
        return Ok(Json(vec![]));
    }
    let limit = req.limit.unwrap_or(50).clamp(1, 200);
    let pattern = format!("%{}%", q.to_lowercase());

    // For each matching message we surface a snippet (240 chars around
    // the first match). Conversations are deduped — only the most
    // recent matching message per conversation comes back.
    let hits: Vec<ConvHit> = sqlx::query_as(
        "WITH ranked AS (
            SELECT
                c.id   AS conversation_id,
                c.project_id,
                p.name AS project_name,
                c.title,
                c.mode,
                m.id   AS message_id,
                m.content,
                m.created_at,
                c.updated_at,
                ROW_NUMBER() OVER (PARTITION BY c.id ORDER BY m.created_at DESC) AS rn
            FROM conversations c
            JOIN projects p ON p.id = c.project_id AND user_can_access_project(p.id, $1, 'viewer')
            LEFT JOIN messages m ON m.conversation_id = c.id
               AND LOWER(m.content) LIKE $2
            WHERE LOWER(c.title) LIKE $2
               OR m.id IS NOT NULL
         )
         SELECT
            conversation_id, project_id, project_name, title, mode,
            message_id,
            CASE WHEN content IS NULL THEN NULL ELSE substring(content for 240) END AS snippet,
            updated_at
         FROM ranked
         WHERE rn = 1
         ORDER BY updated_at DESC
         LIMIT $3",
    )
    .bind(auth_user.id)
    .bind(&pattern)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(hits))
}

// ---------------------------------------------------------------------
// B5: Cross-project code search across project_files.
// ---------------------------------------------------------------------

#[derive(Debug, Serialize, FromRow)]
pub struct FileHit {
    pub project_id: Uuid,
    pub project_name: String,
    pub path: String,
    pub size_bytes: Option<i64>,
}

async fn search_code(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(req): Query<SearchQuery>,
) -> AppResult<Json<Vec<FileHit>>> {
    let q = req.q.trim();
    if q.is_empty() {
        return Ok(Json(vec![]));
    }
    let limit = req.limit.unwrap_or(100).clamp(1, 500);
    let pattern = format!("%{}%", q.to_lowercase());

    let hits: Vec<FileHit> = sqlx::query_as(
        "SELECT
            f.project_id,
            p.name AS project_name,
            f.path,
            f.size_bytes
         FROM project_files f
         JOIN projects p ON p.id = f.project_id AND user_can_access_project(p.id, $1, 'viewer')
         WHERE LOWER(f.path) LIKE $2
         ORDER BY p.name, f.path
         LIMIT $3",
    )
    .bind(auth_user.id)
    .bind(&pattern)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(hits))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/user/tasks",         get(list_user_tasks))
        .route("/user/usage",         get(user_usage))
        .route("/user/debate-health", get(user_debate_health))
        .route("/user/conversations", get(search_conversations))
        .route("/user/code",          get(search_code))
}
