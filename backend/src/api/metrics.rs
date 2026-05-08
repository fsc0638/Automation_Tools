use axum::{
    extract::{Path, State},
    response::Json,
    routing::get,
    Extension, Router,
};
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    error::{AppError, AppResult},
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/projects/:id/metrics/summary", get(metrics_summary))
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
    })))
}
