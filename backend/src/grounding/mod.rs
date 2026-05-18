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

pub mod embedding;
pub mod tools;

use anyhow::{anyhow, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::agents::orchestrator::ProjectScope;
use crate::api::project_index::relevant_file_context;
use crate::crypto::TokenCipher;
use crate::db::models::{GitIdentity, Message, Project};
use crate::git_ops::manager::{sync_current_branch, GitCredentials, SyncResult};
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

// ════════════════════════════════════════════════════════════════════
// Phase 2 — Remote freshness (axis 1: "以遠端 or 本地為基準")
//
// 2a `freshen_local`  : before grounding, best-effort timeout-bounded
//                       `git fetch + ff` so the local clone tracks
//                       origin. Failure is non-fatal — we fall back to
//                       the existing on-disk copy (the plan's
//                       "失敗 fallback 用舊副本").
// 2b `remote_file`    : read a single file by `path@ref` straight from
//                       the GitHub/GitLab Contents API — the
//                       `RemoteLive` path for "查特定 ref 的特定檔",
//                       with a small in-process TTL cache.
// ════════════════════════════════════════════════════════════════════

/// Where the grounded files come from / how fresh they must be.
/// `source` in the architecture plan — the user-facing
/// "以遠端 or 本地為基準" switch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum GroundingSource {
    /// Default. Best-effort `git fetch + ff` of the local clone before
    /// grounding, then ground on local files. Sync failure ⇒ stale copy.
    #[default]
    LocalSynced,
    /// Ground on whatever is on disk now — skip the remote refresh
    /// (offline, or the caller already synced this turn).
    LocalAsIs,
}

/// Outcome of a pre-grounding freshness pass. Purely informational
/// (logging / telemetry / the citation contract). Never an error:
/// freshness is best-effort by design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshenOutcome {
    NotGit,
    Skipped,
    UpToDate,
    FastForwarded,
    NoRemoteBranch,
    TimedOut,
    Failed,
}

impl FreshenOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            FreshenOutcome::NotGit => "not-git",
            FreshenOutcome::Skipped => "skipped",
            FreshenOutcome::UpToDate => "up-to-date",
            FreshenOutcome::FastForwarded => "fast-forwarded",
            FreshenOutcome::NoRemoteBranch => "no-remote-branch",
            FreshenOutcome::TimedOut => "timed-out-stale",
            FreshenOutcome::Failed => "failed-stale",
        }
    }
}

/// Hard ceiling for the pre-grounding sync. A slow/unreachable remote
/// must never stall a chat turn or minutes generation; we bail and use
/// the existing copy instead.
const FRESHEN_TIMEOUT_SECS: u64 = 12;

/// Resolve a project's git credentials by decrypting the linked
/// `git_identity` token. `None` when the project has no identity
/// (public repo / local clone with ambient auth). Centralised here so
/// every grounding caller resolves credentials identically.
pub async fn resolve_project_git_credentials(
    db: &PgPool,
    cipher: &TokenCipher,
    project: &Project,
) -> Option<GitCredentials> {
    let identity_id = project.git_identity_id?;
    let identity: GitIdentity =
        sqlx::query_as("SELECT * FROM git_identities WHERE id = $1")
            .bind(identity_id)
            .fetch_optional(db)
            .await
            .ok()
            .flatten()?;
    let access_token = cipher.decrypt(&identity.access_token).ok()?;
    Some(GitCredentials {
        username: identity.username,
        access_token,
    })
}

/// Phase 2a. Refresh the local clone from origin before grounding.
///
/// Best-effort and timeout-bounded: a non-git project, missing clone,
/// diverged branch, network failure, or timeout all just mean we ground
/// on the existing on-disk copy. The returned [`FreshenOutcome`] is for
/// logging only — callers must not treat it as a hard error.
pub async fn freshen_local(
    project: &Project,
    credentials: Option<GitCredentials>,
    source: &GroundingSource,
) -> FreshenOutcome {
    if *source == GroundingSource::LocalAsIs {
        return FreshenOutcome::Skipped;
    }
    if project.source_type != "git" {
        return FreshenOutcome::NotGit;
    }
    // Mirror project_root_path / build_project_scope: git clone lives at
    // local_path (fallback source_path) — sync exactly what we ground.
    let root = project
        .local_path
        .clone()
        .unwrap_or_else(|| project.source_path.clone());
    if root.trim().is_empty() {
        return FreshenOutcome::NotGit;
    }

    // git2 is blocking; run it off the async runtime and cap the wall
    // clock so a stuck fetch can't wedge the caller.
    let task =
        tokio::task::spawn_blocking(move || sync_current_branch(&root, credentials.as_ref()));
    match tokio::time::timeout(
        std::time::Duration::from_secs(FRESHEN_TIMEOUT_SECS),
        task,
    )
    .await
    {
        Ok(Ok(Ok(SyncResult::AlreadyUpToDate))) => FreshenOutcome::UpToDate,
        Ok(Ok(Ok(SyncResult::FastForwarded))) => FreshenOutcome::FastForwarded,
        Ok(Ok(Ok(SyncResult::NoRemoteBranch))) => FreshenOutcome::NoRemoteBranch,
        Ok(Ok(Err(_))) => FreshenOutcome::Failed, // diverged / fetch error
        Ok(Err(_)) => FreshenOutcome::Failed,     // spawn_blocking join error
        Err(_) => FreshenOutcome::TimedOut,       // exceeded the budget
    }
}

// ── 2b: live remote single-file read ─────────────────────────────────

/// Tiny in-process TTL cache for remote file reads so repeated grounding
/// of the same `path@ref` doesn't hammer the GitHub/GitLab API (the
/// plan's "rate-limit + 短期快取"). std-only; no extra deps.
fn remote_cache(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, String)>>
{
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, String)>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

const REMOTE_CACHE_TTL_SECS: u64 = 60;

/// Parse a GitHub/GitLab clone/browse URL into `(provider, owner, repo)`.
/// Supports `https://host/owner/repo(.git)` and `git@host:owner/repo.git`.
fn parse_repo_url(url: &str) -> Option<(&'static str, String, String)> {
    let u = url.trim();
    let provider = if u.contains("github.com") {
        "github"
    } else if u.contains("gitlab") {
        "gitlab"
    } else {
        return None;
    };
    // Strip scheme / scp-style prefix down to "owner/repo(.git)".
    let tail = u
        .split_once("github.com")
        .or_else(|| u.split_once("gitlab.com"))
        .map(|(_, rest)| rest)
        .unwrap_or(u);
    let tail = tail.trim_start_matches([':', '/']);
    let tail = tail.strip_suffix(".git").unwrap_or(tail);
    let mut parts = tail.split('/').filter(|s| !s.is_empty());
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.to_string();
    Some((provider, owner, repo))
}

/// Phase 2b. Read a single file's text **straight from the remote**
/// (GitHub/GitLab Contents API) at a specific `git_ref` — the
/// `RemoteLive` path for "查特定 ref 的特定檔". Short-term cached.
///
/// Returns the decoded UTF-8 file content. Caller is responsible for
/// running the result through the context firewall (same as any other
/// grounded snippet) — this function only fetches.
pub async fn remote_file(
    project: &Project,
    credentials: Option<&GitCredentials>,
    path: &str,
    git_ref: &str,
) -> Result<String> {
    let (provider, owner, repo) =
        parse_repo_url(&project.source_path).ok_or_else(|| {
            anyhow!("unsupported remote host (only github.com / gitlab.com)")
        })?;
    let path = path.trim_start_matches('/');
    let cache_key = format!("{provider}:{owner}/{repo}:{git_ref}:{path}");

    if let Ok(map) = remote_cache().lock() {
        if let Some((at, body)) = map.get(&cache_key) {
            if at.elapsed().as_secs() < REMOTE_CACHE_TTL_SECS {
                return Ok(body.clone());
            }
        }
    }

    let client = reqwest::Client::new();
    let body = match provider {
        "github" => {
            let url = format!(
                "https://api.github.com/repos/{owner}/{repo}/contents/{path}?ref={git_ref}"
            );
            let mut req = client
                .get(&url)
                .header("User-Agent", "kway-dev-grounding")
                .header("Accept", "application/vnd.github.raw+json");
            if let Some(c) = credentials {
                req = req.bearer_auth(&c.access_token);
            }
            let resp = req.send().await?;
            if !resp.status().is_success() {
                return Err(anyhow!(
                    "GitHub contents API {} for {path}@{git_ref}",
                    resp.status()
                ));
            }
            resp.text().await?
        }
        "gitlab" => {
            // GitLab needs the project path URL-encoded and the file
            // path URL-encoded; ?ref= selects the commit/branch/tag.
            let enc_proj = format!("{owner}%2F{repo}");
            let enc_path = path.replace('/', "%2F");
            let url = format!(
                "https://gitlab.com/api/v4/projects/{enc_proj}/repository/files/{enc_path}/raw?ref={git_ref}"
            );
            let mut req = client.get(&url).header("User-Agent", "kway-dev-grounding");
            if let Some(c) = credentials {
                req = req.header("PRIVATE-TOKEN", c.access_token.clone());
            }
            let resp = req.send().await?;
            if !resp.status().is_success() {
                return Err(anyhow!(
                    "GitLab files API {} for {path}@{git_ref}",
                    resp.status()
                ));
            }
            resp.text().await?
        }
        _ => unreachable!("parse_repo_url restricts provider"),
    };

    if let Ok(mut map) = remote_cache().lock() {
        map.insert(cache_key, (std::time::Instant::now(), body.clone()));
    }
    Ok(body)
}
