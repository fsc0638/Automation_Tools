use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::Response,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

use crate::{
    agents::{
        generic::AgentProfileRuntime,
        orchestrator::{
            build_project_scope, run_agent_stream, strip_role_prefix, AgentMode,
            DebateParticipant, ServerEvent,
        },
    },
    api::{
        auth::verify_token,
        conversation_memory::{
            get_project_summary, load_project_history, refresh_conversation_summary,
            refresh_project_summary,
        },
        project_index::relevant_file_context,
        shared_memory::{truncate_note_bodies, SharedMemoryNote},
        AppState,
    },
    db::models::{AgentProfile, Project},
    error::AppError,
    security::context_firewall::{secure_agent_context, AgentDataPolicy},
};

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub token: String,
    pub conversation_id: Uuid,
    pub project_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientEvent {
    #[serde(rename = "message")]
    Message {
        content: String,
        file_path: Option<String>,
        mode: Option<String>,
    },
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/ws/chat", get(ws_handler))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let claims = verify_token(&query.token, &state.config.jwt_secret)?;
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized("Invalid token".into()))?;

    let conversation_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT c.id
         FROM conversations c
         WHERE c.id = $1 AND c.project_id = $2
           AND (c.user_id = $3 OR user_can_access_project(c.project_id, $3, 'viewer'))",
    )
    .bind(query.conversation_id)
    .bind(query.project_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    if conversation_exists.is_none() {
        return Err(AppError::NotFound("Conversation not found".into()));
    }

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, query, user_id)))
}

async fn handle_socket(socket: WebSocket, state: AppState, query: WsQuery, user_id: Uuid) {
    let (mut sender, mut receiver) = socket.split();

    let project: Project = match sqlx::query_as(
        "SELECT * FROM projects WHERE id = $1 AND user_can_access_project(id, $2, 'viewer')",
    )
    .bind(query.project_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    {
        Ok(project) => project,
        Err(_) => {
            let error = ServerEvent::Error {
                message: "Project not found for agent scope".into(),
            };
            let _ = sender
                .send(WsMessage::Text(
                    serde_json::to_string(&error).unwrap_or_default().into(),
                ))
                .await;
            return;
        }
    };
    let base_project_scope = build_project_scope(&project);

    while let Some(Ok(msg)) = receiver.next().await {
        let text = match msg {
            WsMessage::Text(t) => t,
            WsMessage::Close(_) => break,
            _ => continue,
        };

        let event: ClientEvent = match serde_json::from_str(&text) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let ClientEvent::Message {
            content,
            file_path,
            mode,
        } = event;

        // Load previous history before saving this turn. The orchestrator appends
        // the current user message itself, so this avoids duplicating it in agent context.
        let history = load_project_history(&state.db, query.project_id)
            .await
            .unwrap_or_default();
        let project_summary = get_project_summary(&state.db, query.project_id)
            .await
            .ok()
            .flatten();
        let shared_notes: Vec<SharedMemoryNote> = sqlx::query_as(
            "SELECT * FROM shared_memory_notes
             WHERE user_id = $1
               AND (cardinality(scope_projects) = 0 OR $2 = ANY(scope_projects))
             ORDER BY pinned DESC, updated_at DESC
             LIMIT 20",
        )
        .bind(user_id)
        .bind(query.project_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
        let shared_notes = truncate_note_bodies(shared_notes);
        // Fetch the conversation title for the cost-events snapshot (mig 0023).
        // We grab it per-message rather than once at session start so a
        // rename mid-session is reflected in subsequent rows. Failure
        // collapses to an empty string — the snapshot is best-effort and
        // never blocks the chat.
        let conversation_title_snapshot: String = sqlx::query_scalar(
            "SELECT title FROM conversations WHERE id = $1",
        )
        .bind(query.conversation_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

        // user_id (mig 0021) lets the chat UI display the author's name
        // when the conversation lives in a shared project.
        let user_saved = sqlx::query(
            "INSERT INTO messages (conversation_id, role, content, file_path, user_id)
             VALUES ($1, 'user', $2, $3, $4)",
        )
        .bind(query.conversation_id)
        .bind(&content)
        .bind(&file_path)
        .bind(user_id)
        .execute(&state.db)
        .await;

        if user_saved.is_err() {
            let error = ServerEvent::Error {
                message: "Failed to save user message".into(),
            };
            let _ = sender
                .send(WsMessage::Text(
                    serde_json::to_string(&error).unwrap_or_default().into(),
                ))
                .await;
            continue;
        }

        let _ = sqlx::query("UPDATE conversations SET updated_at = NOW() WHERE id = $1")
            .bind(query.conversation_id)
            .execute(&state.db)
            .await;

        let agent_mode = match agent_mode_from_str(&state, user_id, mode.as_deref()).await {
            Ok(mode) => mode,
            Err(error) => {
                let error = ServerEvent::Error {
                    message: error.to_string(),
                };
                let _ = sender
                    .send(WsMessage::Text(
                        serde_json::to_string(&error).unwrap_or_default().into(),
                    ))
                    .await;
                continue;
            }
        };
        let mut project_scope = base_project_scope.clone();
        project_scope.relevant_file_context =
            relevant_file_context(&state.db, query.project_id, &content)
                .await
                .ok()
                .flatten();

        let mode_label = mode_label(&agent_mode);
        let data_policy = AgentDataPolicy::for_mode(&agent_mode);
        let secured_context = match secure_agent_context(
            &state.db,
            user_id,
            query.project_id,
            query.conversation_id,
            mode_label,
            &data_policy,
            &project_scope,
            &history,
            project_summary.map(|summary| summary.summary),
            &content,
        )
        .await
        {
            Ok(context) => context,
            Err(error) => {
                let error = ServerEvent::Error {
                    message: format!("Context firewall failed: {error}"),
                };
                let _ = sender
                    .send(WsMessage::Text(
                        serde_json::to_string(&error).unwrap_or_default().into(),
                    ))
                    .await;
                continue;
            }
        };
        tracing::info!(
            project_id = %query.project_id,
            conversation_id = %query.conversation_id,
            mode = mode_label,
            redacted_count = secured_context.report.redacted_count,
            blocked_count = secured_context.report.blocked_items.len(),
            classification_max = %secured_context.report.classification_max,
            "context firewall applied"
        );

        let mut stream = run_agent_stream(
            &state.config,
            &secured_context.project_scope,
            &secured_context.history,
            secured_context.project_summary.clone(),
            shared_notes,
            &secured_context.user_message,
            agent_mode,
        );
        let mut buffers: HashMap<String, String> = HashMap::new();
        let mut timing: HashMap<String, AgentCallTiming> = HashMap::new();
        // Track stream outcome so we can flip any pending task_attempts
        // for this conversation to 'complete' or 'failed' once the run
        // finishes. A single conversation can host multiple dispatches
        // over its lifetime; we only flip rows still in 'pending'.
        let mut stream_had_error = false;
        let mut stream_had_done = false;

        while let Some(event) = stream.next().await {
            match &event {
                ServerEvent::Status {
                    agent,
                    round,
                    phase,
                    input_tokens,
                    ..
                } => {
                    // Status fires before chunks; treat it as the call's start
                    // and remember the orchestrator's input-token estimate.
                    let key = event_key(agent, *round, phase.as_deref());
                    let entry = timing.entry(key).or_insert_with(AgentCallTiming::new);
                    if let Some(it) = input_tokens {
                        entry.input_tokens = Some(*it);
                    }
                }
                ServerEvent::Chunk {
                    agent,
                    content,
                    round,
                    phase,
                } => {
                    let key = event_key(agent, *round, phase.as_deref());
                    let entry = timing
                        .entry(key.clone())
                        .or_insert_with(AgentCallTiming::new);
                    if entry.first_chunk_at.is_none() {
                        entry.first_chunk_at = Some(Instant::now());
                    }
                    buffers.entry(key).or_default().push_str(content);
                }
                ServerEvent::Done {
                    agent,
                    round,
                    phase,
                    input_tokens,
                    output_tokens,
                    provider,
                    model,
                } => {
                    let key = event_key(agent, *round, phase.as_deref());
                    let call_timing = timing.remove(&key).map(|mut t| {
                        if let Some(value) = input_tokens {
                            t.exact_input_tokens = Some(*value);
                        }
                        if let Some(value) = output_tokens {
                            t.exact_output_tokens = Some(*value);
                        }
                        if let Some(value) = provider {
                            t.provider = Some(value.clone());
                        }
                        if let Some(value) = model {
                            t.model = Some(value.clone());
                        }
                        t
                    });
                    if let Some(content) = buffers.remove(&key) {
                        if !content.trim().is_empty() {
                            // Models occasionally mimic the `[Hermes]: ...` envelope
                            // we use to label history turns. Strip it before persisting
                            // so chat UI doesn't show the prefix to the user.
                            let content = strip_role_prefix(&content);
                            let role = agent_role(agent);
                            let display_name = display_agent_name(agent, *round, phase.as_deref());
                            let saved_id: Option<(Uuid,)> = sqlx::query_as(
                                "INSERT INTO messages (conversation_id, role, content, agent_name)
                                 VALUES ($1, $2, $3, $4) RETURNING id",
                            )
                            .bind(query.conversation_id)
                            .bind(role)
                            .bind(&content)
                            .bind(&display_name)
                            .fetch_optional(&state.db)
                            .await
                            .ok()
                            .flatten();

                            let _ = sqlx::query(
                                "UPDATE conversations SET updated_at = NOW() WHERE id = $1",
                            )
                            .bind(query.conversation_id)
                            .execute(&state.db)
                            .await;

                            record_usage_event(
                                &state.db,
                                query.project_id,
                                query.conversation_id,
                                user_id,
                                &project.name,
                                &conversation_title_snapshot,
                                saved_id.map(|(id,)| id),
                                role,
                                mode_label,
                                phase.as_deref(),
                                *round,
                                call_timing.as_ref(),
                                &content,
                            )
                            .await;
                        }
                    }
                    stream_had_done = true;
                }
                ServerEvent::Error { message } => {
                    stream_had_error = true;
                    let _ = sqlx::query(
                        "INSERT INTO messages (conversation_id, role, content, agent_name)
                         VALUES ($1, 'system', $2, 'System')",
                    )
                    .bind(query.conversation_id)
                    .bind(message)
                    .execute(&state.db)
                    .await;
                }
            }

            let json = serde_json::to_string(&event).unwrap_or_default();
            if sender.send(WsMessage::Text(json.into())).await.is_err() {
                return;
            }
        }

        // Flip any pending task_attempts on this conversation to
        // 'complete' or 'failed' so the Roadmap drawer's Dispatch
        // History reflects the run outcome. Only rows still pending
        // are touched — re-runs of the same conv only update their
        // own latest attempt. An error anywhere in the stream wins
        // over a partial success.
        if stream_had_error || stream_had_done {
            let new_status = if stream_had_error {
                "failed"
            } else {
                "complete"
            };
            let _ = sqlx::query(
                "UPDATE task_attempts
                 SET status = $1, updated_at = NOW()
                 WHERE conversation_id = $2 AND status = 'pending'",
            )
            .bind(new_status)
            .bind(query.conversation_id)
            .execute(&state.db)
            .await;
        }

        let _ = refresh_project_summary(&state.db, &state.config, &project_scope).await;
        // Per-conversation summary is a UI nice-to-have; ignore failures so
        // they never bubble back to the user (the chat itself already
        // succeeded by this point).
        let _ = refresh_conversation_summary(&state.db, &state.config, query.conversation_id).await;
    }
}

async fn agent_mode_from_str(
    state: &AppState,
    user_id: Uuid,
    mode: Option<&str>,
) -> Result<AgentMode, AppError> {
    let mode = match mode {
        Some("hermes") => AgentMode::HermesOnly,
        Some("debate") => AgentMode::Debate,
        Some(value) if value.starts_with("agents:") => {
            // Each comma-separated token is either the reserved built-in
            // word `openclaw` / `hermes`, or a UUID pointing to one of the
            // user's enabled agent profiles. The frontend picker enforces
            // 2-4 entries up front; we additionally check here so a malformed
            // payload can't drive a one-agent "debate".
            let raw_tokens = value
                .trim_start_matches("agents:")
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .take(4)
                .collect::<Vec<_>>();
            if raw_tokens.len() < 2 {
                return Err(AppError::BadRequest(
                    "Custom debate requires at least 2 participants".into(),
                ));
            }
            let mut participants: Vec<DebateParticipant> = Vec::new();
            for tok in raw_tokens {
                let lower = tok.to_ascii_lowercase();
                if lower == "openclaw" {
                    participants.push(DebateParticipant::OpenClaw);
                } else if lower == "hermes" {
                    participants.push(DebateParticipant::Hermes);
                } else {
                    let profile_id = Uuid::parse_str(tok)
                        .map_err(|_| AppError::BadRequest(format!("Invalid debate participant: {tok}")))?;
                    participants.push(DebateParticipant::Custom(
                        load_agent_profile_runtime(state, user_id, profile_id).await?,
                    ));
                }
            }
            AgentMode::CustomDebate(participants)
        }
        Some(value) if value.starts_with("agent:") => {
            let id = value.trim_start_matches("agent:");
            let profile_id = Uuid::parse_str(id)
                .map_err(|_| AppError::BadRequest("Invalid agent profile id".into()))?;
            AgentMode::Custom(load_agent_profile_runtime(state, user_id, profile_id).await?)
        }
        _ => AgentMode::OpenClawOnly,
    };
    Ok(mode)
}

async fn load_agent_profile_runtime(
    state: &AppState,
    user_id: Uuid,
    profile_id: Uuid,
) -> Result<AgentProfileRuntime, AppError> {
    let profile: AgentProfile = sqlx::query_as(
        "SELECT * FROM agent_profiles WHERE id = $1 AND user_id = $2 AND enabled = TRUE",
    )
    .bind(profile_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Agent profile not found or disabled".into()))?;
    let api_key = state
        .cipher
        .decrypt(&profile.api_key)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("agent key decrypt failed: {}", e)))?;
    Ok(AgentProfileRuntime {
        id: profile.id.to_string(),
        name: profile.name,
        provider: profile.provider,
        model: profile.model,
        base_url: profile.base_url,
        role_prompt: profile.role_prompt,
        api_key,
        allowed_classification_max: profile.allowed_classification_max,
        allow_code_context: profile.allow_code_context,
        allow_project_memory: profile.allow_project_memory,
        allow_conversation_history: profile.allow_conversation_history,
        require_redaction: profile.require_redaction,
        external_processing_allowed: profile.external_processing_allowed,
        retention_policy: profile.retention_policy,
    })
}

fn agent_role(agent: &str) -> &'static str {
    if agent.starts_with("Hermes") {
        "hermes"
    } else {
        "openclaw"
    }
}

fn event_key(agent: &str, round: Option<usize>, phase: Option<&str>) -> String {
    format!(
        "{}:{}:{}",
        agent,
        phase.unwrap_or("single"),
        round.map(|r| r.to_string()).unwrap_or_default()
    )
}

fn display_agent_name(agent: &str, round: Option<usize>, phase: Option<&str>) -> String {
    match (phase, round) {
        (Some("round"), Some(r)) => format!("{agent} · Round {r}"),
        (Some("final"), _) => format!("{agent} · Final"),
        _ => agent.to_string(),
    }
}

fn mode_label(mode: &AgentMode) -> &'static str {
    match mode {
        AgentMode::OpenClawOnly => "openclaw",
        AgentMode::HermesOnly => "hermes",
        AgentMode::Debate => "debate",
        AgentMode::Custom(_) => "custom_agent",
        AgentMode::CustomDebate(_) => "custom_debate",
    }
}

struct AgentCallTiming {
    started_at: Instant,
    first_chunk_at: Option<Instant>,
    input_tokens: Option<u32>,
    exact_input_tokens: Option<u32>,
    exact_output_tokens: Option<u32>,
    provider: Option<String>,
    model: Option<String>,
}

impl AgentCallTiming {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            first_chunk_at: None,
            input_tokens: None,
            exact_input_tokens: None,
            exact_output_tokens: None,
            provider: None,
            model: None,
        }
    }
}

/// Fallback output-token estimate. Prefer provider-reported usage when available.
fn estimate_output_tokens(content: &str) -> i32 {
    let mut cjk: usize = 0;
    let mut other: usize = 0;
    for c in content.chars() {
        if (c as u32) > 0x2E80 {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    (cjk + other / 4) as i32
}

fn resolve_recorded_token_counts(timing: Option<&AgentCallTiming>, content: &str) -> (Option<i32>, i32) {
    let estimated_out = estimate_output_tokens(content);
    match timing {
        Some(t) => (
            t.exact_input_tokens.or(t.input_tokens).map(|v| v as i32),
            t.exact_output_tokens.map(|v| v as i32).unwrap_or(estimated_out),
        ),
        None => (None, estimated_out),
    }
}

fn detect_consensus_marker(content: &str) -> bool {
    content.contains("<!-- consensus:reached -->")
}

/// Detects mentions of a project file path like `src/foo.rs` or `Cargo.toml`.
/// Conservative on extensions to avoid false positives from prose.
fn detect_file_citation(content: &str) -> bool {
    static EXTS: &[&str] = &[
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".md", ".sql", ".toml", ".json", ".yaml",
        ".yml", ".swift", ".kt", ".java", ".go", ".html", ".css",
    ];
    let lower = content.to_lowercase();
    EXTS.iter().any(|ext| lower.contains(ext))
}

#[allow(clippy::too_many_arguments)]
async fn record_usage_event(
    db: &sqlx::PgPool,
    project_id: Uuid,
    conversation_id: Uuid,
    user_id: Uuid,
    project_name_snapshot: &str,
    conversation_title_snapshot: &str,
    message_id: Option<Uuid>,
    agent_role: &str,
    mode: &str,
    phase: Option<&str>,
    round: Option<usize>,
    timing: Option<&AgentCallTiming>,
    content: &str,
) {
    let (ttft_ms, total_ms) = match timing {
        Some(t) => {
            let now = Instant::now();
            let ttft = t
                .first_chunk_at
                .map(|c| c.duration_since(t.started_at).as_millis() as i32);
            let total = now.duration_since(t.started_at).as_millis() as i32;
            (ttft, Some(total))
        }
        None => (None, None),
    };
    let (tokens_in, tokens_out) = resolve_recorded_token_counts(timing, content);
    let provider = timing.and_then(|t| t.provider.as_deref());
    let model = timing.and_then(|t| t.model.as_deref());

    let chars_out = content.chars().count() as i32;

    // mig 0023: denormalize user_id + project/conv labels into the event row
    // so the row survives conv/project deletion (FKs now ON DELETE SET NULL).
    // The snapshot strings can be empty if the conv title fetch failed
    // upstream; readers should COALESCE through current live name → snapshot
    // → "(deleted)" placeholder.
    let _ = sqlx::query(
        "INSERT INTO agent_usage_events (
             project_id, conversation_id, user_id, message_id, agent, mode, phase,
             round_number, ttft_ms, total_ms, tokens_in, tokens_out, chars_out,
             has_consensus_marker, has_file_citation, provider, model,
             project_name_snapshot, conversation_title_snapshot
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)",
    )
    .bind(project_id)
    .bind(conversation_id)
    .bind(user_id)
    .bind(message_id)
    .bind(agent_role)
    .bind(mode)
    .bind(phase)
    .bind(round.map(|r| r as i32))
    .bind(ttft_ms)
    .bind(total_ms)
    .bind(tokens_in)
    .bind(tokens_out)
    .bind(chars_out)
    .bind(detect_consensus_marker(content))
    .bind(detect_file_citation(content))
    .bind(provider)
    .bind(model)
    .bind(project_name_snapshot)
    .bind(conversation_title_snapshot)
    .execute(db)
    .await;
}

#[cfg(test)]
mod tests {
    use super::{resolve_recorded_token_counts, AgentCallTiming};
    use std::time::Instant;

    #[test]
    fn exact_usage_overrides_estimated_tokens() {
        let timing = AgentCallTiming {
            started_at: Instant::now(),
            first_chunk_at: None,
            input_tokens: Some(999),
            exact_input_tokens: Some(321),
            exact_output_tokens: Some(123),
            provider: None,
            model: None,
        };

        let (tokens_in, tokens_out) = resolve_recorded_token_counts(Some(&timing), "這是一段本地估算會不同的內容");
        assert_eq!(tokens_in, Some(321));
        assert_eq!(tokens_out, 123);
    }
}
