use axum::{
    extract::{Path, Query, State},
    response::Json,
    routing::get,
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::collections::BTreeMap;
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
    let exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM projects WHERE id = $1 AND user_can_access_project(id, $2, 'viewer')",
    )
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

    let rows: Vec<(
        chrono::DateTime<chrono::Utc>,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        i64,
    )> = sqlx::query_as(
        "SELECT
            date_trunc('day', created_at) AS day,
            agent,
            mode,
            provider,
            model,
            SUM(tokens_in)::bigint,
            SUM(tokens_out)::bigint,
            COUNT(*)
         FROM agent_usage_events
         WHERE project_id = $1 AND created_at > NOW() - INTERVAL '30 days'
         GROUP BY day, agent, mode, provider, model
         ORDER BY day, agent, mode, provider, model",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    let price_for_event = |agent: &str, provider: Option<&str>, _model: Option<&str>| -> (f64, f64) {
        cfg.price_for(agent, provider)
    };

    let mut total_cost = 0.0_f64;
    let mut by_agent_map: BTreeMap<String, CostBucket> = BTreeMap::new();
    let mut by_mode_map: BTreeMap<String, CostBucket> = BTreeMap::new();
    let mut daily_map: BTreeMap<(String, String), CostBucket> = BTreeMap::new();

    for (day, agent, mode, provider, model, tokens_in, tokens_out, calls) in rows {
        let t_in = tokens_in.unwrap_or(0);
        let t_out = tokens_out.unwrap_or(0);
        let (in_p, out_p) = price_for_event(&agent, provider.as_deref(), model.as_deref());
        let cost = (t_in as f64 / 1000.0) * in_p + (t_out as f64 / 1000.0) * out_p;
        total_cost += cost;

        let by_agent = by_agent_map.entry(agent.clone()).or_default();
        by_agent.tokens_in += t_in;
        by_agent.tokens_out += t_out;
        by_agent.calls += calls;
        by_agent.cost_usd += cost;

        let by_mode = by_mode_map.entry(mode).or_default();
        by_mode.tokens_in += t_in;
        by_mode.tokens_out += t_out;
        by_mode.calls += calls;
        by_mode.cost_usd += cost;

        let daily = daily_map
            .entry((day.format("%Y-%m-%d").to_string(), agent))
            .or_default();
        daily.tokens_in += t_in;
        daily.tokens_out += t_out;
        daily.cost_usd += cost;
    }

    let by_agent: Vec<serde_json::Value> = by_agent_map
        .into_iter()
        .map(|(agent, bucket)| {
            json!({
                "agent": agent,
                "tokens_in": bucket.tokens_in,
                "tokens_out": bucket.tokens_out,
                "calls": bucket.calls,
                "cost_usd": bucket.cost_usd,
            })
        })
        .collect();

    let by_mode: Vec<serde_json::Value> = by_mode_map
        .into_iter()
        .map(|(mode, bucket)| {
            json!({
                "mode": mode,
                "tokens_in": bucket.tokens_in,
                "tokens_out": bucket.tokens_out,
                "calls": bucket.calls,
                "cost_usd": bucket.cost_usd,
            })
        })
        .collect();

    let daily: Vec<serde_json::Value> = daily_map
        .into_iter()
        .map(|((day, agent), bucket)| {
            json!({
                "day": day,
                "agent": agent,
                "tokens_in": bucket.tokens_in,
                "tokens_out": bucket.tokens_out,
                "cost_usd": bucket.cost_usd,
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
        "note": "Prefer gateway/provider-reported usage when available; fallback to local token estimates (CJK ≈ 1 tok, ASCII ≈ 1/4 tok). Mode costs are priced per event before grouping.",
    })))
}

#[derive(Debug, Default, Clone)]
struct CostBucket {
    tokens_in: i64,
    tokens_out: i64,
    calls: i64,
    cost_usd: f64,
}

fn level(score: i32) -> &'static str {
    if score >= 80 { "Low" } else if score >= 50 { "Medium" } else { "High" }
}

fn clamp_score(value: i32) -> i32 {
    value.clamp(0, 100)
}

fn ratio_pct(part: i64, total: i64) -> f64 {
    if total <= 0 { 0.0 } else { (part as f64 / total as f64) * 100.0 }
}

fn confidence_from_inventory(total_files: i64, source_files: i64) -> i32 {
    if total_files <= 0 {
        0
    } else if total_files >= 2000 {
        // The index currently caps at 2,000 files, so broad repos need an
        // explicit full scan before we should claim near-certainty.
        82
    } else if source_files >= 10 {
        92
    } else if source_files > 0 {
        85
    } else {
        70
    }
}

async fn metrics_health(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    verify_project_access(&state.db, project_id, auth_user.id).await?;
    let db = &state.db;

    let files: Vec<(String, Option<String>, i64)> = sqlx::query_as(
        "SELECT path, language, size_bytes FROM project_files WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    let total_files = files.len() as i64;
    let source_exts = [
        "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "swift", "php", "rb",
        "cs", "cpp", "c", "h", "hpp",
    ];
    let source_files = files
        .iter()
        .filter(|(path, lang, _)| {
            let lang = lang.as_deref().unwrap_or_default();
            source_exts.contains(&lang) || source_exts.iter().any(|ext| path.ends_with(&format!(".{ext}")))
        })
        .count() as i64;
    let test_files = files
        .iter()
        .filter(|(path, _, _)| {
            let p = path.to_lowercase();
            p.contains("test") || p.contains("spec") || p.contains("__tests__") || p.contains(".test.")
        })
        .count() as i64;
    let readme_count = files
        .iter()
        .filter(|(path, _, _)| {
            let p = path.to_lowercase();
            p == "readme.md" || p.starts_with("readme") || p.contains("/readme")
        })
        .count() as i64;
    let docs_count = files
        .iter()
        .filter(|(path, _, _)| {
            let p = path.to_lowercase();
            p.starts_with("docs/") || p.contains("/docs/")
        })
        .count() as i64;
    let markdown_count = files
        .iter()
        .filter(|(path, lang, _)| lang.as_deref() == Some("md") || path.to_lowercase().ends_with(".md"))
        .count() as i64;
    let config_count = files
        .iter()
        .filter(|(path, _, _)| {
            matches!(
                path.as_str(),
                "package.json" | "Cargo.toml" | "pyproject.toml" | "requirements.txt" | "go.mod" |
                "pom.xml" | "build.gradle" | "docker-compose.yml" | "Dockerfile"
            ) || path.ends_with("/package.json") || path.ends_with("/Cargo.toml") || path.ends_with("/go.mod")
        })
        .count() as i64;
    let ci_count = files
        .iter()
        .filter(|(path, _, _)| {
            let p = path.to_lowercase();
            p.starts_with(".github/workflows/") || p.contains("/workflows/") || p.contains("gitlab-ci")
                || p.contains("azure-pipelines") || p.contains("circleci")
        })
        .count() as i64;
    let env_example_count = files
        .iter()
        .filter(|(path, _, _)| {
            let p = path.to_lowercase();
            p.ends_with(".env.example") || p.ends_with(".env.sample") || p.contains("env.example")
        })
        .count() as i64;
    let lockfile_count = files
        .iter()
        .filter(|(path, _, _)| {
            matches!(
                path.as_str(),
                "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "Cargo.lock" | "poetry.lock" | "Gemfile.lock" | "go.sum"
            ) || path.ends_with("/package-lock.json") || path.ends_with("/Cargo.lock") || path.ends_with("/go.sum")
        })
        .count() as i64;
    let max_size = files.iter().map(|(_, _, size)| *size).max().unwrap_or(0);
    let avg_size = if total_files > 0 {
        files.iter().map(|(_, _, size)| *size as f64).sum::<f64>() / total_files as f64
    } else {
        0.0
    };
    let large_files = files.iter().filter(|(_, _, size)| *size > 100_000).count() as i64;
    let huge_files = files.iter().filter(|(_, _, size)| *size > 500_000).count() as i64;
    let top_dirs = files
        .iter()
        .filter_map(|(path, _, _)| path.split('/').next())
        .filter(|part| !part.is_empty())
        .collect::<std::collections::HashSet<_>>()
        .len() as i64;

    let secret_hits: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM project_file_chunks
         WHERE project_id = $1
           AND path !~* '(^|/)\\.env\\.example$|(^|/)\\.env\\.sample$|example|sample|readme|docs/'
           AND content ~ '(AKIA[0-9A-Z]{16}|sk-[A-Za-z0-9_-]{20,}|-----BEGIN [A-Z ]*PRIVATE KEY-----|(?i)(password|api[_-]?key|secret|token)\\s*[:=]\\s*[\"''][^\"'']{8,})'",
    )
    .bind(project_id)
    .fetch_one(db)
    .await?;
    let dependency_risk_hits: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM project_file_chunks
         WHERE project_id = $1
           AND path ~* '(package\\.json|cargo\\.toml|requirements\\.txt|pyproject\\.toml|go\\.mod|pom\\.xml)'
           AND LOWER(content) ~ '(deprecated|unmaintained|vulnerab|audit|override|resolution)'",
    )
    .bind(project_id)
    .fetch_one(db)
    .await?;
    let test_script_hits: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM project_file_chunks
         WHERE project_id = $1
           AND path ~* '(package\\.json|cargo\\.toml|pyproject\\.toml|makefile|justfile)'
           AND LOWER(content) ~ '(\"test\"|cargo test|pytest|vitest|jest|go test|npm test)'",
    )
    .bind(project_id)
    .fetch_one(db)
    .await?;
    let doc_run_hits: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM project_file_chunks
         WHERE project_id = $1
           AND path ~* '(readme|docs/|\\.md$)'
           AND LOWER(content) ~ '(install|setup|getting started|quickstart|run|build|test|deploy|configuration|environment|??|??|??|??|??)'",
    )
    .bind(project_id)
    .fetch_one(db)
    .await?;

    let inventory_confidence = confidence_from_inventory(total_files, source_files);

    // Architecture: structural proxy based only on indexed file inventory.
    // It intentionally does not reward silence from agents. Unknown evidence lowers confidence.
    let large_ratio = ratio_pct(large_files, total_files);
    let mut arch_score = 100;
    if total_files == 0 { arch_score = 0; }
    if config_count == 0 { arch_score -= 18; }
    if top_dirs <= 1 && source_files > 20 { arch_score -= 12; }
    if large_ratio > 2.0 { arch_score -= 12; }
    if large_ratio > 5.0 { arch_score -= 12; }
    if huge_files > 0 { arch_score -= 16; }
    if source_files > 300 && top_dirs < 4 { arch_score -= 12; }
    let arch_score = clamp_score(arch_score);
    let arch_confidence = if total_files == 0 { 0 } else { inventory_confidence.min(92) };

    // Maintenance: file count, size distribution, dependency/CI evidence.
    let mut maint_score = 100;
    if total_files == 0 { maint_score = 0; }
    if total_files > 500 { maint_score -= 10; }
    if total_files > 2000 { maint_score -= 18; }
    if avg_size > 10_000.0 { maint_score -= 10; }
    if max_size > 100_000 { maint_score -= 12; }
    if max_size > 500_000 { maint_score -= 18; }
    if ci_count == 0 && source_files > 20 { maint_score -= 10; }
    if lockfile_count == 0 && config_count > 0 { maint_score -= 8; }
    let maint_score = clamp_score(maint_score);
    let maint_confidence = if total_files == 0 { 0 } else { inventory_confidence.max(90).min(96) };

    // Tests: test readiness, not exact code coverage. Exact coverage requires executing project test tooling.
    let test_ratio = if source_files > 0 { test_files as f64 / source_files as f64 } else { 0.0 };
    let mut test_score = ((test_ratio * 400.0).min(70.0)) as i32;
    if test_script_hits.0 > 0 { test_score += 20; }
    if ci_count > 0 && test_files > 0 { test_score += 10; }
    if source_files == 0 { test_score = 0; }
    let test_score = clamp_score(test_score);
    let test_confidence = if total_files == 0 { 0 } else if test_script_hits.0 > 0 { 93 } else { 88 };

    // Documentation: existence + actionable run/build/test/deploy instructions.
    let mut doc_score = 0;
    if readme_count > 0 { doc_score += 30; }
    if docs_count > 0 { doc_score += 20; }
    if markdown_count >= 5 { doc_score += 15; } else if markdown_count > 0 { doc_score += 8; }
    if doc_run_hits.0 > 0 { doc_score += 25; }
    if env_example_count > 0 { doc_score += 10; }
    if total_files == 0 { doc_score = 0; }
    let doc_score = clamp_score(doc_score);
    let doc_confidence = if total_files == 0 { 0 } else { 94 };

    // Security: concrete secret-pattern and dependency-risk evidence only. This is not a full SAST scan.
    let mut sec_score = 100;
    if total_files == 0 { sec_score = 0; }
    if secret_hits.0 > 0 { sec_score -= (secret_hits.0 as i32 * 25).min(75); }
    if dependency_risk_hits.0 > 0 { sec_score -= (dependency_risk_hits.0 as i32 * 8).min(24); }
    if env_example_count == 0 && source_files > 20 { sec_score -= 8; }
    let sec_score = clamp_score(sec_score);
    let sec_confidence = if total_files == 0 { 0 } else { 91 };

    let dimension_scores = [arch_score, maint_score, test_score, doc_score, sec_score];
    let dimension_confidences = [arch_confidence, maint_confidence, test_confidence, doc_confidence, sec_confidence];
    let composite = dimension_scores.iter().sum::<i32>() / dimension_scores.len() as i32;
    let confidence = dimension_confidences.iter().sum::<i32>() / dimension_confidences.len() as i32;

    let dimensions = json!([
        {
            "key": "architecture",
            "label": "????",
            "score": arch_score,
            "level": level(arch_score),
            "confidence": arch_confidence,
            "measured_by": "Indexed file inventory: source/config distribution, top-level module spread, large-file and huge-file ratios.",
            "formula": "100 - missing_config(18) - single_top_dir_with_many_sources(12) - large_file_ratio_penalties(12/24) - huge_file_penalty(16) - large_repo_low_module_spread(12)",
            "evidence": format!("{} indexed files, {} source files, {} config/entry files, {} top-level dirs, {} large files, {} huge files", total_files, source_files, config_count, top_dirs, large_files, huge_files),
            "evidence_items": [
                format!("config_or_entry_files={}", config_count),
                format!("top_level_dirs={}", top_dirs),
                format!("large_file_ratio={:.1}%", large_ratio),
                format!("huge_files_gt_500kb={}", huge_files)
            ]
        },
        {
            "key": "maintenance",
            "label": "????",
            "score": maint_score,
            "level": level(maint_score),
            "confidence": maint_confidence,
            "measured_by": "Indexed file count, average/max file size, CI workflow evidence, dependency lockfile evidence.",
            "formula": "100 - file_count_penalties - avg/max_size_penalties - missing_ci(10 when source_files>20) - missing_lockfile(8 when dependency manifest exists)",
            "evidence": format!("{} files, avg {} bytes, max {} bytes, CI files {}, lockfiles {}", total_files, avg_size as i64, max_size, ci_count, lockfile_count),
            "evidence_items": [
                format!("total_files={}", total_files),
                format!("avg_size_bytes={}", avg_size as i64),
                format!("max_size_bytes={}", max_size),
                format!("ci_workflows={}", ci_count),
                format!("lockfiles={}", lockfile_count)
            ]
        },
        {
            "key": "tests",
            "label": "????",
            "score": test_score,
            "level": level(test_score),
            "confidence": test_confidence,
            "measured_by": "Test file naming evidence, test command evidence in manifests, and CI test-readiness signal. This is not executed coverage.",
            "formula": "min(test_files/source_files*400, 70) + test_script_present(20) + ci_with_tests(10)",
            "evidence": format!("{} test-like files / {} source files, test scripts {}, CI files {}", test_files, source_files, test_script_hits.0, ci_count),
            "evidence_items": [
                format!("test_files={}", test_files),
                format!("source_files={}", source_files),
                format!("test_script_hits={}", test_script_hits.0),
                format!("ci_workflows={}", ci_count)
            ]
        },
        {
            "key": "docs",
            "label": "?????",
            "score": doc_score,
            "level": level(doc_score),
            "confidence": doc_confidence,
            "measured_by": "README/docs/Markdown inventory plus actionable setup/run/test/deploy/configuration wording in documentation chunks.",
            "formula": "README(30) + docs_dir(20) + markdown_volume(8/15) + actionable_docs(25) + env_example(10)",
            "evidence": format!("README {}, docs files {}, markdown files {}, actionable doc hits {}, env examples {}", readme_count, docs_count, markdown_count, doc_run_hits.0, env_example_count),
            "evidence_items": [
                format!("readme_count={}", readme_count),
                format!("docs_count={}", docs_count),
                format!("markdown_count={}", markdown_count),
                format!("actionable_doc_hits={}", doc_run_hits.0),
                format!("env_examples={}", env_example_count)
            ]
        },
        {
            "key": "security",
            "label": "????",
            "score": sec_score,
            "level": level(sec_score),
            "confidence": sec_confidence,
            "measured_by": "Concrete secret-pattern scan over indexed chunks, dependency-risk wording in manifests, and env-example presence. This is not full SAST/DAST.",
            "formula": "100 - secret_pattern_hits*25 capped at 75 - dependency_risk_hits*8 capped at 24 - missing_env_example(8 when source_files>20)",
            "evidence": format!("{} possible secret pattern hits, {} dependency risk hints, env examples {}", secret_hits.0, dependency_risk_hits.0, env_example_count),
            "evidence_items": [
                format!("possible_secret_hits={}", secret_hits.0),
                format!("dependency_risk_hits={}", dependency_risk_hits.0),
                format!("env_examples={}", env_example_count)
            ]
        },
    ]);

    Ok(Json(json!({
        "score": composite,
        "confidence": confidence,
        "methodology": "Evidence-based MVP: scores are calculated from indexed repository files/chunks and do not use agent opinion as health evidence. Unknown evidence lowers confidence instead of being treated as healthy.",
        "limitations": [
            "Architecture score is a structural proxy until AST dependency graph and circular dependency detection are added.",
            "Test score measures test readiness, not executed line/branch coverage.",
            "Security score scans indexed text for high-signal patterns, not a complete SAST/dependency audit.",
            "The current file index caps at 2,000 files and 1MB per indexed file. Very large repos need a full scanner for 90+ confidence."
        ],
        "dimensions": dimensions,
        "indexed_files": total_files,
        "signals": {
            "source_files": source_files,
            "test_files": test_files,
            "config_files": config_count,
            "ci_files": ci_count,
            "lockfiles": lockfile_count,
            "readme_files": readme_count,
            "docs_files": docs_count,
            "markdown_files": markdown_count,
            "possible_secret_hits": secret_hits.0,
            "dependency_risk_hits": dependency_risk_hits.0
        }
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
