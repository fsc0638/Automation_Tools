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
    config::Config,
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
    /// True when the original project has been deleted and only a
    /// `project_name_snapshot` remains. Frontend uses this to render
    /// the row in a muted / strikethrough style. (mig 0023)
    #[serde(default)]
    pub project_deleted: bool,
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

/// Per-token pricing helper. Mirrors `metrics.rs::price_for_event` so the
/// global Insights page agrees with the per-project Insights tab on cost
/// math. Custom agent profiles fall back to the OpenClaw price table for
/// now — `cost_per_1k` lookup by `provider`/`model` is tracked separately.
fn cost_for(cfg: &Config, agent: &str, tokens_in: i64, tokens_out: i64) -> f64 {
    let (in_p, out_p) = if agent == "hermes" {
        (cfg.hermes_price_per_1k_in, cfg.hermes_price_per_1k_out)
    } else {
        (cfg.openclaw_price_per_1k_in, cfg.openclaw_price_per_1k_out)
    };
    (tokens_in as f64 / 1000.0) * in_p + (tokens_out as f64 / 1000.0) * out_p
}

async fn user_usage(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(q): Query<UsageQuery>,
) -> AppResult<Json<UserUsage>> {
    let days = clamp_days(q.days);
    let cfg = state.config.as_ref();

    // `days` is the only string-interpolated value in this module's SQL;
    // it is clamped to [1, 365] above, so no injection surface. We can't
    // parameterise an INTERVAL literal directly with sqlx, and rebuilding
    // each query with `make_interval($n)` adds noise without changing
    // semantics for an internal clamped int.
    //
    // Schema reality (from migration 0007 + 0022): agent_usage_events
    // stores `tokens_in INT`, `tokens_out INT`, and now `provider`/`model`.
    // There is NO `cost_usd` column; cost is computed Rust-side from
    // per-1k token prices the same way metrics.rs::cost_summary does it.

    // ----- Raw rows: (project_id_opt, project_name, agent, tokens_in, tokens_out, calls) -----
    //
    // mig 0023: events now carry e.user_id directly, so we filter the
    // user's own spend without going through user_can_access_project().
    // That keeps history visible even when the project (or the user's
    // ACL on it) has since been deleted. project_id can be NULL when the
    // project itself is gone — we fall back to project_name_snapshot
    // and a synthesized zero-uuid so the rest of the rollup keeps a
    // stable key per "logical project".
    let raw_sql = format!(
        "SELECT
            COALESCE(p.id, '00000000-0000-0000-0000-000000000000'::uuid) AS project_id,
            COALESCE(p.name, e.project_name_snapshot, '(deleted project)') AS project_name,
            (p.id IS NULL)                                                AS project_deleted,
            e.agent                                                       AS agent,
            COALESCE(SUM(e.tokens_in),  0)::int8                          AS tokens_in,
            COALESCE(SUM(e.tokens_out), 0)::int8                          AS tokens_out,
            COUNT(*)::int8                                                AS calls
         FROM agent_usage_events e
         LEFT JOIN projects p ON p.id = e.project_id
         WHERE e.user_id = $1
           AND e.created_at >= NOW() - INTERVAL '{days} days'
         GROUP BY COALESCE(p.id, '00000000-0000-0000-0000-000000000000'::uuid),
                  COALESCE(p.name, e.project_name_snapshot, '(deleted project)'),
                  (p.id IS NULL),
                  e.agent"
    );
    let rows: Vec<(Uuid, String, bool, String, i64, i64, i64)> = sqlx::query_as(&raw_sql)
        .bind(auth_user.id)
        .fetch_all(&state.db)
        .await?;

    // Build by_project and by_agent rollups in one pass.
    use std::collections::HashMap;
    let mut project_map: HashMap<Uuid, ProjectUsage> = HashMap::new();
    let mut agent_map: HashMap<String, AgentUsage> = HashMap::new();

    for (project_id, project_name, project_deleted, agent, tokens_in, tokens_out, calls) in rows {
        let cost = cost_for(cfg, &agent, tokens_in, tokens_out);
        let p = project_map.entry(project_id).or_insert(ProjectUsage {
            project_id,
            project_name,
            project_deleted,
            calls: 0,
            tokens_in: 0,
            tokens_out: 0,
            cost_usd: 0.0,
        });
        p.calls += calls;
        p.tokens_in += tokens_in;
        p.tokens_out += tokens_out;
        p.cost_usd += cost;

        let a = agent_map.entry(agent.clone()).or_insert(AgentUsage {
            agent,
            calls: 0,
            tokens_in: 0,
            tokens_out: 0,
            cost_usd: 0.0,
        });
        a.calls += calls;
        a.tokens_in += tokens_in;
        a.tokens_out += tokens_out;
        a.cost_usd += cost;
    }

    // Include projects with zero usage so the side-bar listing stays
    // stable across windows (e.g. you can still click into a project
    // that didn't get any agent calls in the last N days).
    let zero_projects: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT p.id, p.name FROM projects p
         WHERE user_can_access_project(p.id, $1, 'viewer')",
    )
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;
    for (id, name) in zero_projects {
        project_map.entry(id).or_insert(ProjectUsage {
            project_id: id,
            project_name: name,
            project_deleted: false,
            calls: 0,
            tokens_in: 0,
            tokens_out: 0,
            cost_usd: 0.0,
        });
    }

    let mut by_project: Vec<ProjectUsage> = project_map.into_values().collect();
    by_project.sort_by(|a, b| {
        b.cost_usd
            .partial_cmp(&a.cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.calls.cmp(&a.calls))
    });

    let mut by_agent: Vec<AgentUsage> = agent_map.into_values().collect();
    by_agent.sort_by(|a, b| {
        b.cost_usd
            .partial_cmp(&a.cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.calls.cmp(&a.calls))
    });

    let total_calls: i64 = by_project.iter().map(|r| r.calls).sum();
    let total_tokens_in: i64 = by_project.iter().map(|r| r.tokens_in).sum();
    let total_tokens_out: i64 = by_project.iter().map(|r| r.tokens_out).sum();
    let total_cost_usd: f64 = by_project.iter().map(|r| r.cost_usd).sum();

    // ----- Daily trend: same raw shape but bucketed by day + agent so
    //       the cost helper can be applied identically. Filter by
    //       e.user_id (mig 0023) so historical days from deleted projects
    //       still show up in the user's own time series. -----
    let daily_rows_sql = format!(
        "SELECT
            DATE_TRUNC('day', e.created_at)::date AS day,
            e.agent                               AS agent,
            COALESCE(SUM(e.tokens_in),  0)::int8  AS tokens_in,
            COALESCE(SUM(e.tokens_out), 0)::int8  AS tokens_out,
            COUNT(*)::int8                        AS calls
         FROM agent_usage_events e
         WHERE e.user_id = $1
           AND e.created_at >= NOW() - INTERVAL '{days} days'
         GROUP BY 1, 2
         ORDER BY 1"
    );
    let daily_rows: Vec<(NaiveDate, String, i64, i64, i64)> =
        sqlx::query_as(&daily_rows_sql)
            .bind(auth_user.id)
            .fetch_all(&state.db)
            .await?;

    let mut daily_map: std::collections::BTreeMap<NaiveDate, (i64, f64)> =
        std::collections::BTreeMap::new();
    for (day, agent, tokens_in, tokens_out, calls) in daily_rows {
        let cost = cost_for(cfg, &agent, tokens_in, tokens_out);
        let entry = daily_map.entry(day).or_insert((0, 0.0));
        entry.0 += calls;
        entry.1 += cost;
    }
    let daily: Vec<JsonValue> = daily_map
        .into_iter()
        .map(|(day, (calls, cost_usd))| {
            serde_json::json!({
                "day": day.format("%Y-%m-%d").to_string(),
                "calls": calls,
                "cost_usd": cost_usd,
            })
        })
        .collect();

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
    /// True when the project was deleted but historical debate events
    /// remain (project_name comes from the snapshot column). mig 0023.
    #[serde(default)]
    pub project_deleted: bool,
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

    // Per-project rollup: count debate turns, consensus, file-citation.
    // mig 0023: filter by e.user_id (LEFT JOIN projects for live name),
    // so deleted projects still surface their historical debate metrics
    // via project_name_snapshot.
    let by_project_sql = format!(
        "SELECT
            COALESCE(p.id, '00000000-0000-0000-0000-000000000000'::uuid) AS project_id,
            COALESCE(p.name, e.project_name_snapshot, '(deleted project)') AS project_name,
            (p.id IS NULL)                                                AS project_deleted,
            COUNT(*) FILTER (WHERE e.mode = 'debate')::int8               AS debate_turns,
            COUNT(*) FILTER (
                WHERE e.mode = 'debate' AND e.has_consensus_marker
            )::int8                                                       AS consensus_turns,
            COUNT(*) FILTER (
                WHERE e.has_file_citation
            )::int8                                                       AS citation_turns
         FROM agent_usage_events e
         LEFT JOIN projects p ON p.id = e.project_id
         WHERE e.user_id = $1
           AND e.created_at >= NOW() - INTERVAL '{days} days'
         GROUP BY COALESCE(p.id, '00000000-0000-0000-0000-000000000000'::uuid),
                  COALESCE(p.name, e.project_name_snapshot, '(deleted project)'),
                  (p.id IS NULL)
         ORDER BY debate_turns DESC, project_name"
    );
    let by_project: Vec<ProjectDebateHealth> = sqlx::query_as(&by_project_sql)
        .bind(auth_user.id)
        .fetch_all(&state.db)
        .await?;

    // Round distribution: same e.user_id-direct filter; no project join
    // needed because we don't display the project here.
    let round_sql = format!(
        "SELECT
            COALESCE(e.round_number, 0)::int4 AS rounds,
            COUNT(*)::int8                    AS count
         FROM agent_usage_events e
         WHERE e.user_id = $1
           AND e.mode = 'debate'
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
