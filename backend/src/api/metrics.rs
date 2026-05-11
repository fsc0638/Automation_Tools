use axum::{
    extract::{Path, Query, State},
    response::Json,
    routing::get,
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    error::{AppError, AppResult},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/projects/:id/metrics/summary", get(metrics_summary))
        .route("/projects/:id/metrics/cost", get(metrics_cost))
        .route("/projects/:id/metrics/health", get(metrics_health))
        .route("/projects/:id/metrics/burndown", get(metrics_burndown))
}

#[derive(Debug, Serialize)]
struct ModeCount {
    mode: String,
    count: i64,
}

#[derive(Debug, Serialize)]
struct AgentAvg {
    agent: String,
    avg_chars: f64,
    response_count: i64,
}

#[derive(Debug, Serialize)]
struct RoundBucket {
    round: i32,
    count: i64,
}

async fn verify_project_access(
    db: &PgPool,
    project_id: Uuid,
    user_id: Uuid,
) -> AppResult<()> {
    let exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM projects WHERE id = $1 AND user_id = $2")
            .bind(project_id)
            .bind(user_id)
            .fetch_optional(db)
            .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("Project not found".into()));
    }
    Ok(())
}

async fn metrics_summary(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    verify_project_access(&state.db, project_id, auth_user.id).await?;
    let db = &state.db;

    // Totals: conversations, messages, by-role
    let totals: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT COUNT(*) FROM conversations WHERE project_id = $1),
            (SELECT COUNT(*) FROM messages m
                JOIN conversations c ON c.id = m.conversation_id
                WHERE c.project_id = $1),
            (SELECT COUNT(*) FROM messages m
                JOIN conversations c ON c.id = m.conversation_id
                WHERE c.project_id = $1 AND m.role = 'user'),
            (SELECT COUNT(*) FROM messages m
                JOIN conversations c ON c.id = m.conversation_id
                WHERE c.project_id = $1 AND m.role IN ('openclaw','hermes'))
        ",
    )
    .bind(project_id)
    .fetch_one(db)
    .await?;

    // Mode distribution: how many conversations per mode
    let mode_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT mode, COUNT(*) FROM conversations WHERE project_id = $1 GROUP BY mode",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;
    let mode_distribution: Vec<ModeCount> = mode_rows
        .into_iter()
        .map(|(mode, count)| ModeCount { mode, count })
        .collect();

    // Average reply length per agent (last 90 days, agent role only)
    let agent_rows: Vec<(String, Option<f64>, i64)> = sqlx::query_as(
        "SELECT m.role, AVG(LENGTH(m.content)::float8), COUNT(*)
         FROM messages m
         JOIN conversations c ON c.id = m.conversation_id
         WHERE c.project_id = $1
           AND m.role IN ('openclaw','hermes')
           AND m.created_at > NOW() - INTERVAL '90 days'
         GROUP BY m.role",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;
    let avg_chars_by_agent: Vec<AgentAvg> = agent_rows
        .into_iter()
        .map(|(agent, avg, count)| AgentAvg {
            agent,
            avg_chars: avg.unwrap_or(0.0),
            response_count: count,
        })
        .collect();

    // Consensus rate: across debate "final" events, how many had the marker
    let consensus_row: (i64, i64) = sqlx::query_as(
        "SELECT
            COUNT(*) FILTER (WHERE mode = 'debate' AND phase = 'final'),
            COUNT(*) FILTER (WHERE mode = 'debate' AND has_consensus_marker)
         FROM agent_usage_events WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_one(db)
    .await?;
    let consensus_finals = consensus_row.0;
    let consensus_with = consensus_row.1;
    let consensus_rate = if consensus_finals > 0 {
        consensus_with as f64 / consensus_finals as f64
    } else {
        0.0
    };

    // Debate round distribution: bucket by max round per conversation/turn
    let round_rows: Vec<(Option<i32>, i64)> = sqlx::query_as(
        "SELECT round_number, COUNT(*) FROM agent_usage_events
         WHERE project_id = $1 AND mode = 'debate' AND phase = 'round'
         GROUP BY round_number ORDER BY round_number",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;
    let debate_round_distribution: Vec<RoundBucket> = round_rows
        .into_iter()
        .filter_map(|(r, count)| r.map(|round| RoundBucket { round, count }))
        .collect();

    // Timing: avg / p50 / p95 across all agent calls
    let timing_row: (Option<f64>, Option<f64>, Option<f64>, Option<f64>) = sqlx::query_as(
        "SELECT
            AVG(ttft_ms)::float8,
            AVG(total_ms)::float8,
            PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY total_ms)::float8,
            PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY total_ms)::float8
         FROM agent_usage_events WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_one(db)
    .await?;

    // Per-agent feedback counts (👍 = +1, 👎 = -1)
    let feedback_rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT m.role,
                COUNT(*) FILTER (WHERE f.rating = 1),
                COUNT(*) FILTER (WHERE f.rating = -1)
         FROM message_feedback f
         JOIN messages m ON m.id = f.message_id
         JOIN conversations c ON c.id = m.conversation_id
         WHERE c.project_id = $1 AND m.role IN ('openclaw','hermes')
         GROUP BY m.role",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;
    let feedback_by_agent: Vec<serde_json::Value> = feedback_rows
        .into_iter()
        .map(|(agent, ups, downs)| {
            let total = ups + downs;
            let satisfaction = if total > 0 { ups as f64 / total as f64 } else { 0.0 };
            json!({
                "agent": agent,
                "thumbs_up": ups,
                "thumbs_down": downs,
                "total": total,
                "satisfaction_rate": satisfaction,
            })
        })
        .collect();

    // File citation rate
    let citation_row: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE has_file_citation)
         FROM agent_usage_events WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_one(db)
    .await?;
    let citation_total = citation_row.0;
    let citation_with = citation_row.1;
    let citation_rate = if citation_total > 0 {
        citation_with as f64 / citation_total as f64
    } else {
        0.0
    };

    Ok(Json(json!({
        "totals": {
            "conversations": totals.0,
            "messages": totals.1,
            "user_messages": totals.2,
            "agent_messages": totals.3,
        },
        "mode_distribution": mode_distribution,
        "avg_chars_by_agent": avg_chars_by_agent,
        "consensus": {
            "debate_finals": consensus_finals,
            "with_consensus": consensus_with,
            "rate": consensus_rate,
        },
        "debate_round_distribution": debate_round_distribution,
        "timing": {
            "avg_ttft_ms": timing_row.0,
            "avg_total_ms": timing_row.1,
            "p50_total_ms": timing_row.2,
            "p95_total_ms": timing_row.3,
        },
        "file_citation": {
            "total": citation_total,
            "with_citation": citation_with,
            "rate": citation_rate,
        },
        "feedback_by_agent": feedback_by_agent,
    })))
}

async fn metrics_cost(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    verify_project_access(&state.db, project_id, auth_user.id).await?;
    let db = &state.db;
    let cfg = &state.config;

    // Per-agent token totals over the past 30 days.
    let agent_rows: Vec<(String, Option<i64>, Option<i64>, i64)> = sqlx::query_as(
        "SELECT agent, SUM(tokens_in)::bigint, SUM(tokens_out)::bigint, COUNT(*)
         FROM agent_usage_events
         WHERE project_id = $1 AND created_at > NOW() - INTERVAL '30 days'
         GROUP BY agent",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    // Per-mode token totals over the past 30 days.
    let mode_rows: Vec<(String, Option<i64>, Option<i64>, i64)> = sqlx::query_as(
        "SELECT mode, SUM(tokens_in)::bigint, SUM(tokens_out)::bigint, COUNT(*)
         FROM agent_usage_events
         WHERE project_id = $1 AND created_at > NOW() - INTERVAL '30 days'
         GROUP BY mode",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    // Daily breakdown for trend chart — includes input + output cost per agent.
    let daily_rows: Vec<(chrono::DateTime<chrono::Utc>, String, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT date_trunc('day', created_at) AS day, agent,
                SUM(tokens_in)::bigint, SUM(tokens_out)::bigint
         FROM agent_usage_events
         WHERE project_id = $1 AND created_at > NOW() - INTERVAL '30 days'
         GROUP BY day, agent
         ORDER BY day",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    let price_for = |agent: &str| -> (f64, f64) {
        if agent == "hermes" {
            (cfg.hermes_price_per_1k_in, cfg.hermes_price_per_1k_out)
        } else {
            (cfg.openclaw_price_per_1k_in, cfg.openclaw_price_per_1k_out)
        }
    };

    let mut total_cost = 0.0_f64;
    let by_agent: Vec<serde_json::Value> = agent_rows
        .into_iter()
        .map(|(agent, tokens_in, tokens_out, calls)| {
            let t_in = tokens_in.unwrap_or(0);
            let t_out = tokens_out.unwrap_or(0);
            let (in_p, out_p) = price_for(&agent);
            let cost = (t_in as f64 / 1000.0) * in_p + (t_out as f64 / 1000.0) * out_p;
            total_cost += cost;
            json!({
                "agent": agent,
                "tokens_in": t_in,
                "tokens_out": t_out,
                "calls": calls,
                "cost_usd": cost,
            })
        })
        .collect();

    let by_mode: Vec<serde_json::Value> = mode_rows
        .into_iter()
        .map(|(mode, tokens_in, tokens_out, calls)| {
            let t_in = tokens_in.unwrap_or(0);
            let t_out = tokens_out.unwrap_or(0);
            // Mode rows mix agents, so use OpenClaw pricing as a proxy.
            let cost = (t_in as f64 / 1000.0) * cfg.openclaw_price_per_1k_in
                + (t_out as f64 / 1000.0) * cfg.openclaw_price_per_1k_out;
            json!({
                "mode": mode,
                "tokens_in": t_in,
                "tokens_out": t_out,
                "calls": calls,
                "cost_usd": cost,
            })
        })
        .collect();

    let daily: Vec<serde_json::Value> = daily_rows
        .into_iter()
        .map(|(day, agent, tokens_in, tokens_out)| {
            let t_in = tokens_in.unwrap_or(0);
            let t_out = tokens_out.unwrap_or(0);
            let (in_p, out_p) = price_for(&agent);
            let cost = (t_in as f64 / 1000.0) * in_p + (t_out as f64 / 1000.0) * out_p;
            json!({
                "day": day.format("%Y-%m-%d").to_string(),
                "agent": agent,
                "tokens_in": t_in,
                "tokens_out": t_out,
                "cost_usd": cost,
            })
        })
        .collect();

    Ok(Json(json!({
        "by_agent": by_agent,
        "by_mode": by_mode,
        "daily": daily,
        "total_cost_usd": total_cost,
        "pricing": {
            "openclaw_per_1k_in": cfg.openclaw_price_per_1k_in,
            "openclaw_per_1k_out": cfg.openclaw_price_per_1k_out,
            "hermes_per_1k_in": cfg.hermes_price_per_1k_in,
            "hermes_per_1k_out": cfg.hermes_price_per_1k_out,
        },
        "note": "Tokens estimated locally (CJK ≈ 1 tok, ASCII ≈ 1/4 tok). Wire gateway-reported usage in a future phase for exact billing.",
    })))
}

fn level(score: i32) -> &'static str {
    if score >= 80 { "Low" } else if score >= 50 { "Medium" } else { "High" }
}

async fn metrics_health(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    verify_project_access(&state.db, project_id, auth_user.id).await?;
    let db = &state.db;

    // --- 1. Test coverage gap ---
    let file_stats: (i64, i64, Option<i64>, Option<f64>) = sqlx::query_as(
        "SELECT
            COUNT(*),
            COUNT(*) FILTER (
                WHERE path ILIKE '%test%' OR path ILIKE '%spec%'
                   OR path ILIKE '%__tests__%' OR path ILIKE '%.test.%'
            ),
            MAX(size_bytes),
            AVG(size_bytes)::float8
         FROM project_files WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_one(db)
    .await?;
    let total_files = file_stats.0;
    let test_files = file_stats.1;
    let max_size = file_stats.2.unwrap_or(0);
    let avg_size = file_stats.3.unwrap_or(0.0);

    let test_score: i32 = if total_files == 0 {
        50 // unknown — neutral
    } else {
        let ratio = test_files as f64 / total_files as f64;
        // 20% coverage = 100 score, 0% = 0
        ((ratio * 500.0).min(100.0)) as i32
    };

    // --- 2. Documentation ---
    let docs_row: (i64, i64, i64) = sqlx::query_as(
        "SELECT
            COUNT(*) FILTER (WHERE path ILIKE 'README%' OR path ILIKE '%/README%'),
            COUNT(*) FILTER (WHERE path ILIKE 'docs/%' OR path ILIKE '%/docs/%'),
            COUNT(*) FILTER (WHERE language = 'markdown' OR path ILIKE '%.md')
         FROM project_files WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_one(db)
    .await?;
    let has_readme = docs_row.0 > 0;
    let has_docs_dir = docs_row.1 > 0;
    let md_count = docs_row.2;

    let mut doc_score = 0i32;
    if has_readme { doc_score += 40; }
    if has_docs_dir { doc_score += 30; }
    if md_count >= 5 { doc_score += 30; } else if md_count >= 1 { doc_score += 15; }
    if total_files == 0 { doc_score = 50; } // unknown

    // --- 3. Maintenance ---
    // Penalise huge max files / many files / fat average files.
    let mut maint = 100i32;
    if max_size > 100_000 { maint -= 20; }
    if max_size > 500_000 { maint -= 20; }
    if avg_size > 10_000.0 { maint -= 15; }
    if total_files > 500 { maint -= 15; }
    if total_files > 2000 { maint -= 15; }
    let maint_score = maint.max(0);

    // --- 4 & 5. Architecture & security risk via agent message scan ---
    // Higher mention count = lower score (more risk).
    let risk_row: (i64, i64) = sqlx::query_as(
        "SELECT
            COUNT(*) FILTER (WHERE m.role IN ('openclaw','hermes')
                AND (LOWER(m.content) ~ '(race condition|deadlock|tight coupling|circular dependency|architectural risk|架構風險)')),
            COUNT(*) FILTER (WHERE m.role IN ('openclaw','hermes')
                AND (LOWER(m.content) ~ '(vulnerability|injection|secret|credential leak|安全風險|資安)'))
         FROM messages m
         JOIN conversations c ON c.id = m.conversation_id
         WHERE c.project_id = $1",
    )
    .bind(project_id)
    .fetch_one(db)
    .await?;
    let arch_mentions = risk_row.0;
    let sec_mentions = risk_row.1;

    let arch_score: i32 = (100 - (arch_mentions as i32 * 8)).max(0);
    let sec_score: i32 = (100 - (sec_mentions as i32 * 12)).max(0);

    // Composite — equal-weight average for now.
    let composite = (test_score + doc_score + maint_score + arch_score + sec_score) / 5;

    let dimensions = json!([
        {
            "key": "architecture",
            "label": "架構風險",
            "score": arch_score,
            "level": level(arch_score),
            "evidence": format!("{} mention(s) of architectural risk in agent replies", arch_mentions),
        },
        {
            "key": "maintenance",
            "label": "維護成本",
            "score": maint_score,
            "level": level(maint_score),
            "evidence": format!("{} files, avg {} bytes, max {} bytes", total_files, avg_size as i64, max_size),
        },
        {
            "key": "tests",
            "label": "測試缺口",
            "score": test_score,
            "level": level(test_score),
            "evidence": format!("{}/{} files look test-related", test_files, total_files),
        },
        {
            "key": "docs",
            "label": "文件完整度",
            "score": doc_score,
            "level": level(doc_score),
            "evidence": format!("README: {}, docs/: {}, .md count: {}", has_readme, has_docs_dir, md_count),
        },
        {
            "key": "security",
            "label": "安全風險",
            "score": sec_score,
            "level": level(sec_score),
            "evidence": format!("{} mention(s) of security risk in agent replies", sec_mentions),
        },
    ]);

    Ok(Json(json!({
        "score": composite,
        "dimensions": dimensions,
        "indexed_files": total_files,
    })))
}

#[derive(Debug, Serialize)]
struct BurndownPoint {
    /// ISO `YYYY-MM-DD`.
    day: String,
    /// Cumulative tasks created by EOD `day`.
    total: i64,
    /// Cumulative tasks that reached status='done' by EOD `day`. Uses
    /// task_status_history (the audit log added in P1) for the actual
    /// transition time, and falls back to project_tasks.updated_at for
    /// pre-history tasks that have status='done'.
    done: i64,
    /// Tasks still open by EOD `day` (= total - done). Convenience for
    /// the "remaining" line on the chart.
    remaining: i64,
    /// Linear ideal trajectory — full scope at the first day, drops to 0
    /// at the last day. Frontend renders this as a dashed reference line.
    ideal: f64,
}

#[derive(Debug, Serialize)]
struct BurndownResponse {
    points: Vec<BurndownPoint>,
    /// Final scope (total tasks today). Useful for the chart's y-axis cap.
    final_total: i64,
    /// Final remaining (open) tasks.
    final_remaining: i64,
    /// Average tasks closed per day over the window (velocity).
    velocity_per_day: f64,
}

#[derive(Debug, Deserialize, Default)]
pub struct BurndownQuery {
    /// Scope to a single sprint (UUID) or "none" for backlog-only.
    /// Omit / "all" to include every task in the project.
    pub sprint_id: Option<String>,
    /// Window in days, clamped 7..=180. Default 60.
    pub days: Option<i64>,
}

async fn metrics_burndown(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
    Query(query): Query<BurndownQuery>,
) -> AppResult<Json<BurndownResponse>> {
    verify_project_access(&state.db, project_id, auth_user.id).await?;
    let db = &state.db;

    let days = query.days.unwrap_or(60).clamp(7, 180);
    let (sprint_filter, sprint_bind): (&str, Option<Uuid>) = match query.sprint_id.as_deref() {
        Some("none") | Some("backlog") => (" AND t.sprint_id IS NULL", None),
        Some(other) if !other.is_empty() && other != "all" => {
            let id = Uuid::parse_str(other)
                .map_err(|_| AppError::BadRequest("invalid sprint_id".into()))?;
            (" AND t.sprint_id = $3", Some(id))
        }
        _ => ("", None),
    };

    // Build SQL with the optional sprint filter inlined into both CTE
    // references so totals + completion both respect the scope.
    let sql = format!(
        "WITH bounds AS (
            SELECT
                GREATEST(
                    COALESCE(MIN(created_at)::date, CURRENT_DATE),
                    CURRENT_DATE - ($2 || ' days')::interval
                )::date AS start_day,
                CURRENT_DATE AS end_day
            FROM project_tasks t
            WHERE t.project_id = $1{sprint_filter}
         ),
         days AS (
            SELECT generate_series(start_day, end_day, INTERVAL '1 day')::date AS day
            FROM bounds
         ),
         task_done_at AS (
            SELECT
                t.id,
                t.created_at,
                COALESCE(
                    (SELECT MAX(h.changed_at)
                       FROM task_status_history h
                       WHERE h.task_id = t.id AND h.new_status = 'done'),
                    CASE WHEN t.status = 'done' THEN t.updated_at ELSE NULL END
                ) AS done_at
            FROM project_tasks t
            WHERE t.project_id = $1{sprint_filter}
         )
         SELECT
            d.day,
            (SELECT COUNT(*)::int8 FROM task_done_at t WHERE t.created_at::date <= d.day)                                                AS total,
            (SELECT COUNT(*)::int8 FROM task_done_at t WHERE t.done_at IS NOT NULL AND t.done_at::date <= d.day)                          AS done
         FROM days d
         ORDER BY d.day"
    );

    let mut q = sqlx::query_as::<_, (chrono::NaiveDate, i64, i64)>(&sql)
        .bind(project_id)
        .bind(days);
    if let Some(s) = sprint_bind {
        q = q.bind(s);
    }
    let rows = q.fetch_all(db).await?;

    let n = rows.len();
    if n == 0 {
        return Ok(Json(BurndownResponse {
            points: vec![],
            final_total: 0,
            final_remaining: 0,
            velocity_per_day: 0.0,
        }));
    }

    let final_total = rows.last().map(|r| r.1).unwrap_or(0);
    let final_done = rows.last().map(|r| r.2).unwrap_or(0);
    let final_remaining = final_total - final_done;

    // Ideal line: start at final_total (the eventual scope) and linearly
    // drop to 0 at the last day. Indexing-safe even when n == 1.
    let scope = final_total as f64;
    let denom = (n.saturating_sub(1)).max(1) as f64;

    let points: Vec<BurndownPoint> = rows
        .iter()
        .enumerate()
        .map(|(i, &(day, total, done))| {
            let ideal = scope * (1.0 - (i as f64) / denom);
            BurndownPoint {
                day: day.to_string(),
                total,
                done,
                remaining: total - done,
                ideal,
            }
        })
        .collect();

    // Velocity = sum of done deltas / days. Use windowed diffs so a
    // single point window doesn't divide by zero.
    let velocity_per_day = if n < 2 {
        0.0
    } else {
        let mut closed: i64 = 0;
        for win in rows.windows(2) {
            let d = win[1].2 - win[0].2;
            if d > 0 {
                closed += d;
            }
        }
        closed as f64 / (n - 1) as f64
    };

    Ok(Json(BurndownResponse {
        points,
        final_total,
        final_remaining,
        velocity_per_day,
    }))
}
