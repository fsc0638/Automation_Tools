//! Unified AI grounding provider.
//!
//! Single chokepoint that turns (project, query, policy, history) into a
//! firewalled, evidence-grounded context for an agent call. Every flow
//! that talks to Hermes / OpenClaw should ground through here so the
//! pipeline — local snapshot, retrieval, DLP firewall, audit — is
//! implemented and reasoned about exactly once.
//!
//! ## Phase 1 contract (this file's current scope)
//!
//! Behaviour is **byte-identical** to the inline sequence the chat
//! WebSocket handler used to run:
//!
//! ```text
//!   base_scope.clone()
//!     → scope.relevant_file_context = relevant_file_context(query)
//!     → secure_agent_context(scope, history, summary, user_msg)
//! ```
//!
//! It is a pure refactor: no new behaviour, no new context sources. The
//! caller still builds the base [`ProjectScope`] once (filesystem
//! snapshot is comparatively expensive) and hands it in; we clone it and
//! attach the per-turn lexical retrieval, then run the context firewall.
//!
//! ## Why a module now (before it does anything new)
//!
//! Phases 2–5 (remote freshness, hybrid lexical+vector retrieval,
//! meeting-flow grounding, ReAct tool feedback) all extend *this*
//! function. Establishing the seam first — with the chat flow proven to
//! behave identically through it — means later phases change one place
//! and every consumer benefits, instead of each flow re-implementing
//! grounding and drifting apart (which is exactly the state Phase 0
//! found).

use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::agents::orchestrator::ProjectScope;
use crate::api::project_index::relevant_file_context;
use crate::db::models::Message;
use crate::security::context_firewall::{
    secure_agent_context, AgentDataPolicy, SecuredAgentContext,
};

/// Everything the provider needs to assemble one grounded context.
///
/// `base_scope` is the project filesystem snapshot built once per
/// session by the caller (`build_project_scope`). We clone it per call
/// and overlay the per-query retrieval — this preserves the original
/// ws.rs timing (snapshot built once, retrieval refreshed every turn)
/// so Phase 1 is a true no-op refactor.
pub struct GroundingInputs<'a> {
    pub db: &'a PgPool,
    pub user_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    /// Human-readable agent mode label, e.g. "openclaw" / "hermes" /
    /// "debate" — only used for the firewall audit row.
    pub mode_label: &'a str,
    pub data_policy: &'a AgentDataPolicy,
    /// Snapshot built once via `build_project_scope`. Cloned here.
    pub base_scope: &'a ProjectScope,
    pub history: &'a [Message],
    pub project_summary: Option<String>,
    /// The user's message — doubles as the retrieval query and the
    /// firewall-redacted user content.
    pub query: &'a str,
}

/// Assemble grounded + firewalled context for an agent call.
///
/// Equivalent to the old ws.rs inline block; returns the same
/// [`SecuredAgentContext`] the caller already consumed.
pub async fn assemble(input: GroundingInputs<'_>) -> Result<SecuredAgentContext> {
    // 1. Per-turn lexical retrieval overlaid on the session snapshot.
    let mut scope = input.base_scope.clone();
    scope.relevant_file_context =
        relevant_file_context(input.db, input.project_id, input.query)
            .await
            .ok()
            .flatten();

    // 2. DLP context firewall (classification gate + secret redaction +
    //    audit row). Unchanged from before — just relocated.
    secure_agent_context(
        input.db,
        input.user_id,
        input.project_id,
        input.conversation_id,
        input.mode_label,
        input.data_policy,
        &scope,
        input.history,
        input.project_summary,
        input.query,
    )
    .await
}
