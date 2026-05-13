use anyhow::Result;
use chrono::Utc;
use futures_util::{Stream, StreamExt};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Default maximum time to wait for the next visible streamed token before
/// treating an agent as stalled. This is configurable because Hermes may spend
/// longer thinking in late debate rounds with large context.
const DEFAULT_STREAM_CHUNK_TIMEOUT: Duration = Duration::from_secs(300);
const DEBATE_NOTES_FILE: &str = "CONVERSATION_NOTES_DEBATE.md";

/// Batch upstream tokens into chunks of at least this many bytes before
/// forwarding to the WebSocket. Reduces ServerEvent volume from ~1/token
/// (often 30-60/sec) to ~1/sentence, cutting frontend re-render frequency.
const STREAM_BATCH_BYTES: usize = 80;

use crate::{
    agents::{
        generic::{AgentProfileRuntime, GenericAgentClient},
        hermes::HermesClient,
        openclaw::{ChatMessage, OpenClawClient},
        telemetry::{AgentResponseMetadata, AgentStreamEvent},
    },
    config::Config,
    db::models::{Message, Project},
};

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type")]
pub enum ServerEvent {
    #[serde(rename = "status")]
    Status {
        agent: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        round: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        phase: Option<String>,
        /// Estimated input tokens for the call about to start. Phase 6 telemetry
        /// — gives the dashboard a usable input-cost number even though we
        /// don't yet ingest exact `usage` from the gateway.
        #[serde(skip_serializing_if = "Option::is_none")]
        input_tokens: Option<u32>,
    },
    #[serde(rename = "chunk")]
    Chunk {
        agent: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        round: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        phase: Option<String>,
    },
    #[serde(rename = "done")]
    Done {
        agent: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        round: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        phase: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        input_tokens: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_tokens: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Debug, Clone)]
pub enum AgentMode {
    HermesOnly,
    OpenClawOnly,
    Debate,
    Custom(AgentProfileRuntime),
    /// Custom-shaped debate that may mix built-in OpenClaw / Hermes with any
    /// number of user-defined agent profiles. The frontend's debate picker
    /// can include up to 4 participants in any combination, so this used to
    /// require ≥2 custom profiles; now even a single user can run a
    /// "self-test" between only OpenClaw and Hermes through this code path
    /// if they want the picker UX without owning custom agents.
    CustomDebate(Vec<DebateParticipant>),
}

/// One slot in a CustomDebate roster. Lets the orchestrator dispatch each
/// turn to the right client (OpenClawClient / HermesClient / GenericAgent)
/// while keeping the rest of the debate logic agnostic.
#[derive(Debug, Clone)]
pub enum DebateParticipant {
    /// Built-in OpenClaw — uses OPENCLAW_API_URL / OPENCLAW_MODEL from env.
    OpenClaw,
    /// Built-in Hermes — uses HERMES_API_URL / HERMES_MODEL from env.
    Hermes,
    /// A user-defined agent profile dispatched through GenericAgentClient.
    Custom(AgentProfileRuntime),
}

impl DebateParticipant {
    /// Human-visible label rendered in the chat bubble. Matches what the
    /// CustomDebatePicker shows in its row so the round attribution stays
    /// consistent end-to-end.
    pub fn display_name(&self) -> String {
        match self {
            DebateParticipant::OpenClaw => "OpenClaw".to_string(),
            DebateParticipant::Hermes => "Hermes".to_string(),
            DebateParticipant::Custom(profile) => profile.name.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProjectScope {
    pub id: Uuid,
    pub name: String,
    pub source_type: String,
    pub root: Option<String>,
    pub file_snapshot: Option<String>,
    pub relevant_file_context: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DebateAgent {
    OpenClaw,
    Hermes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DebateIntent {
    CasualChat,
    QuickAgreement,
    DebateRequired,
    ImplementationTask,
}

impl DebateAgent {
    fn name(self) -> &'static str {
        match self {
            DebateAgent::OpenClaw => "OpenClaw",
            DebateAgent::Hermes => "Hermes",
        }
    }

    fn role(self) -> &'static str {
        match self {
            DebateAgent::OpenClaw => "openclaw",
            DebateAgent::Hermes => "hermes",
        }
    }

    fn other(self) -> Self {
        match self {
            DebateAgent::OpenClaw => DebateAgent::Hermes,
            DebateAgent::Hermes => DebateAgent::OpenClaw,
        }
    }
}

pub fn build_project_scope(project: &Project) -> ProjectScope {
    let root = if project.source_type == "git" || project.source_type == "upload" {
        project
            .local_path
            .clone()
            .unwrap_or_else(|| project.source_path.clone())
    } else {
        project.source_path.clone()
    };
    let absolute_root = absolutize_path(&root);
    let file_snapshot = absolute_root.as_deref().and_then(build_file_snapshot);

    ProjectScope {
        id: project.id,
        name: project.name.clone(),
        source_type: project.source_type.clone(),
        root: absolute_root.or(Some(root)),
        file_snapshot,
        relevant_file_context: None,
    }
}

fn absolutize_path(path: &str) -> Option<String> {
    let path = PathBuf::from(path);
    let candidate = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    candidate
        .canonicalize()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

fn build_file_snapshot(root: &str) -> Option<String> {
    let root_path = Path::new(root);
    if !root_path.exists() {
        return None;
    }

    let mut tree = Vec::new();
    collect_tree(root_path, root_path, 0, &mut tree);

    let mut important = String::new();
    for file in important_files(root_path) {
        if let Ok(content) = std::fs::read_to_string(&file) {
            let rel = file
                .strip_prefix(root_path)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            let snippet: String = content.chars().take(8_000).collect();
            important.push_str(&format!("\n\n--- FILE: {rel} ---\n{snippet}"));
        }
        if important.len() > 32_000 {
            break;
        }
    }

    Some(format!(
        "Current project file snapshot. This is the only project-specific file evidence available to the agents.\n\n[File tree, truncated]\n{}\n{}",
        tree.join("\n"),
        important
    ))
}

fn collect_tree(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
    if depth > 4 || out.len() >= 180 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries = entries.filter_map(|e| e.ok()).collect::<Vec<_>>();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        if out.len() >= 180 {
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if should_ignore(&name) {
            continue;
        }
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        out.push(format!(
            "{}{}{}",
            "  ".repeat(depth),
            rel,
            if is_dir { "/" } else { "" }
        ));
        if is_dir {
            collect_tree(root, &path, depth + 1, out);
        }
    }
}

fn should_ignore(name: &str) -> bool {
    name == ".git"
        || name == "node_modules"
        || name == "target"
        || name == ".next"
        || name == "dist"
        || name == "build"
        || name == ".venv"
        || name == "__pycache__"
        || (name.starts_with('.') && name != ".env.example")
}

fn important_files(root: &Path) -> Vec<PathBuf> {
    let candidates = [
        "README.md",
        "readme.md",
        "package.json",
        "Cargo.toml",
        "pyproject.toml",
        "requirements.txt",
        "go.mod",
        "pom.xml",
        "build.gradle",
        "docker-compose.yml",
        "Dockerfile",
        "src/main.rs",
        "src/main.ts",
        "src/main.tsx",
        "src/index.ts",
        "src/index.tsx",
        "src/App.tsx",
        "main.py",
        "app.py",
    ];
    candidates
        .iter()
        .map(|p| root.join(p))
        .filter(|p| p.is_file())
        .collect()
}

fn project_scope_message(project: &ProjectScope) -> ChatMessage {
    let root = project.root.as_deref().unwrap_or("not provided");
    ChatMessage {
        role: "system".into(),
        content: format!(
            "Project isolation boundary: Current project id={}, name='{}', source_type='{}', root='{}'. \
             Use ONLY this current project's conversation history and files as project-specific memory. \
             Never import assumptions, answers, file paths, bugs, architecture, or decisions from another project. \
             Cross-project sharing is allowed only for general agent skills, coding patterns, debate habits, and broad engineering knowledge. \
             If the file snapshot and indexed excerpts below are missing or insufficient, explicitly say the project files are unavailable/insufficient. \
             When giving concrete recommendations, cite the most relevant file paths from the snapshot or indexed excerpts. \
             Do not invent architecture from the project name.\n\n{}\n\n{}",
            project.id, project.name, project.source_type, root,
            project.file_snapshot.as_deref().unwrap_or("[No project file snapshot available]"),
            project.relevant_file_context.as_deref().unwrap_or("[No indexed relevant file excerpts selected for this turn]")
        ),
    }
}

fn project_summary_message(summary: &str) -> ChatMessage {
    ChatMessage {
        role: "system".into(),
        content: format!(
            "Shared project memory summary across all conversations/modes for this project. \
             This is lower priority than the current user question. Use it only when it is directly relevant to the current question. \
             Never let this summary change the topic, replace the user's request, or make the Final answer about an older discussion. \
             Prefer it over agent-to-agent chatter only when recalling directly relevant project decisions, constraints, and unresolved items.\n\n{}",
            summary
        ),
    }
}

fn messages_to_chat(
    project: &ProjectScope,
    history: &[Message],
    project_summary: Option<&str>,
) -> Vec<ChatMessage> {
    let mut messages = vec![project_scope_message(project)];
    if let Some(summary) = project_summary.filter(|s| !s.trim().is_empty()) {
        messages.push(project_summary_message(summary));
    }
    messages.extend(history.iter().map(|m| {
        let role = if m.role == "user" {
            "user"
        } else {
            "assistant"
        };
        // Attribution without a mimic-prone `[Role]:` label: use a minimal
        // XML-style tag that models treat as structural metadata rather than
        // text to copy.  `strip_role_prefix` in the output path handles any
        // legacy rows that still carry the old `[Hermes]:` envelope.
        let content = match m.role.as_str() {
            "openclaw" => format!("<agent:openclaw>{}</agent:openclaw>", m.content),
            "hermes" => format!("<agent:hermes>{}</agent:hermes>", m.content),
            "system" => m.content.clone(),
            _ => m.content.clone(),
        };

        ChatMessage {
            role: role.into(),
            content,
        }
    }));
    messages
}

/// Strip a leading role-tag prefix (`[Hermes]:`, `[OpenClaw]:`, `[System]:`)
/// from agent output. The model occasionally mimics the `[role]: ...` envelope
/// used in `messages_to_chat` when constructing history. Tolerates leading
/// whitespace, surrounding quotes/asterisks, and lowercase variants. Only
/// removes ONE prefix so we don't accidentally eat legitimate `[Hermes]:`
/// occurrences mid-message.
pub fn strip_role_prefix(content: &str) -> String {
    let prefixes = [
        "[Hermes]:",
        "[OpenClaw]:",
        "[System]:",
        "[hermes]:",
        "[openclaw]:",
        "[system]:",
        "**[Hermes]:**",
        "**[OpenClaw]:**",
        "**[System]:**",
        "Hermes:",
        "OpenClaw:",
    ];
    let trimmed = content.trim_start();
    for p in prefixes.iter() {
        if let Some(rest) = trimmed.strip_prefix(p) {
            // preserve internal newlines but drop one space after the prefix
            return rest.trim_start_matches(' ').to_string();
        }
    }
    content.to_string()
}

fn choose_debate_lead(history: &[Message], user_message: &str) -> DebateAgent {
    let text = format!(
        "{}\n{}",
        history
            .iter()
            .rev()
            .take(8)
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        user_message
    )
    .to_lowercase();

    let openclaw_keywords = [
        "架構",
        "設計",
        "規劃",
        "計畫",
        "技術選型",
        "大型",
        "複雜",
        "系統",
        "重構",
        "race",
        "deadlock",
        "concurrency",
        "async",
        "間歇",
        "難題",
        "根因",
        "root cause",
        "security",
        "安全",
        "migration",
        "資料模型",
        "跨",
        "影響範圍",
        "邊界",
        "scal",
        "performance",
        "效能",
        "memory",
        "debug 難",
        "難 bug",
    ];
    let hermes_keywords = [
        "寫",
        "新增",
        "修改",
        "調整",
        "修正",
        "一般",
        "簡單",
        "快速",
        "日常",
        "樣式",
        "ui",
        "copy",
        "文字",
        "按鈕",
        "表單",
        "lint",
        "format",
        "小改",
        "直接修",
    ];

    let openclaw_score = openclaw_keywords
        .iter()
        .filter(|kw| text.contains(**kw))
        .count();
    let hermes_score = hermes_keywords
        .iter()
        .filter(|kw| text.contains(**kw))
        .count();

    if openclaw_score > hermes_score || text.len() > 700 {
        DebateAgent::OpenClaw
    } else {
        DebateAgent::Hermes
    }
}

fn classify_debate_intent(user_message: &str) -> DebateIntent {
    let text = user_message.to_lowercase();

    let explicit_debate = [
        "辯論",
        "debate",
        "互相反駁",
        "吵一下",
        "挑戰彼此",
        "兩派",
        "方案 a",
        "方案a",
        "方案 b",
        "方案b",
        "取捨",
        "tradeoff",
        "比較兩個方案",
        "哪個方案",
        "架構選型",
        "技術選型",
    ];
    if explicit_debate.iter().any(|kw| text.contains(kw)) {
        return DebateIntent::DebateRequired;
    }

    if code_change_requested(user_message) {
        return DebateIntent::ImplementationTask;
    }

    let casual_chat = [
        "三個人的聊天",
        "三人聊天",
        "聊一下",
        "討論一下",
        "問一下",
        "你們覺得",
        "你們怎麼看",
        "想聽你們",
        "不用改",
        "先不要改",
        "先聊",
        "只是問",
        "確認一下",
    ];
    if casual_chat.iter().any(|kw| text.contains(kw)) {
        return DebateIntent::CasualChat;
    }

    let quick_agreement = [
        "可以嗎",
        "對嗎",
        "是不是",
        "是否",
        "這樣行嗎",
        "這樣可以",
        "有沒有問題",
        "確認",
        "檢查一下",
    ];
    if quick_agreement.iter().any(|kw| text.contains(kw)) || text.chars().count() < 80 {
        return DebateIntent::QuickAgreement;
    }

    DebateIntent::DebateRequired
}

fn debate_round_limit_label(max_rounds: usize) -> String {
    format!(
        "up to {max_rounds} agent turns, then stop and return unresolved disagreements to the user"
    )
}

fn debate_round_limit(config: &Config) -> usize {
    // 0 = "effectively unlimited" — rely on consensus_reached() and the
    // model's own loop-detection prompt to converge instead of a hard cap.
    // The 50-turn ceiling is a safety net for cost/runtime, not a target.
    if config.debate_max_rounds == 0 {
        50
    } else {
        config.debate_max_rounds.clamp(2, 50)
    }
}

fn stream_chunk_timeout(config: &Config) -> Duration {
    let secs = config.agent_stream_chunk_timeout_secs;
    if secs == 0 {
        DEFAULT_STREAM_CHUNK_TIMEOUT
    } else {
        Duration::from_secs(secs.clamp(30, 900))
    }
}

/// Rough token estimate for a chat array. CJK characters ~1 token each,
/// other characters ~1/4 token. Mirrors the output-side estimator in ws.rs
/// so input + output billing comparisons stay consistent.
fn estimate_chat_tokens(chat: &[ChatMessage]) -> u32 {
    let mut cjk: usize = 0;
    let mut other: usize = 0;
    for msg in chat {
        for c in msg.content.chars() {
            if (c as u32) > 0x2E80 {
                cjk += 1;
            } else {
                other += 1;
            }
        }
    }
    (cjk + other / 4) as u32
}

/// Wrap an LLM token stream so it emits coalesced chunks of at least
/// STREAM_BATCH_BYTES (final partial chunk is always flushed at end).
fn batch_chunks(
    stream: Pin<Box<dyn Stream<Item = AgentStreamEvent> + Send>>,
) -> Pin<Box<dyn Stream<Item = AgentStreamEvent> + Send>> {
    Box::pin(async_stream::stream! {
        let mut buf = String::new();
        let mut s = stream;
        while let Some(item) = s.next().await {
            match item {
                AgentStreamEvent::Content(chunk) => {
                    buf.push_str(&chunk);
                    if buf.len() >= STREAM_BATCH_BYTES {
                        yield AgentStreamEvent::Content(std::mem::take(&mut buf));
                    }
                }
                AgentStreamEvent::Metadata(metadata) => {
                    if !buf.is_empty() {
                        yield AgentStreamEvent::Content(std::mem::take(&mut buf));
                    }
                    yield AgentStreamEvent::Metadata(metadata);
                }
            }
        }
        if !buf.is_empty() {
            yield AgentStreamEvent::Content(buf);
        }
    })
}

fn done_event(
    agent: String,
    round: Option<usize>,
    phase: Option<String>,
    metadata: Option<AgentResponseMetadata>,
) -> ServerEvent {
    ServerEvent::Done {
        agent,
        round,
        phase,
        input_tokens: metadata.as_ref().and_then(|m| m.input_tokens),
        output_tokens: metadata.as_ref().and_then(|m| m.output_tokens),
        provider: metadata.as_ref().map(|m| m.provider.clone()),
        model: metadata.map(|m| m.model),
    }
}

fn debate_instruction(
    lead: DebateAgent,
    round_limit: usize,
    auto_consensus: bool,
    code_change_requested: bool,
    user_message: &str,
) -> ChatMessage {
    let lead_reason = match lead {
        DebateAgent::OpenClaw => "OpenClaw/GPT-5.5 leads first because the task appears complex, strategic, architectural, or high-risk.",
        DebateAgent::Hermes => "Hermes/GPT-5.4 leads first because the task appears implementation-oriented, routine, or suited to quick iteration.",
    };
    let final_rule = if code_change_requested {
        "The user appears to need code changes. Debate rounds are analysis only; the Final answer is the authoritative implementation plan/diff/test instruction. Do not present intermediate debate text as final code to apply."
    } else {
        "The Final answer is still authoritative; debate rounds are for disagreement, risk discovery, and refinement."
    };
    let round_limit_label = debate_round_limit_label(round_limit);

    ChatMessage {
        role: "system".into(),
        content: format!(
            "Debate Mode rules: OpenClaw and Hermes are fixed, independent individuals. \
             Default language is Traditional Chinese; keep code/API/error text unchanged. \
             They must not merge identities. They may challenge, disagree, and correct each other. \
             Do not treat compromise, fatigue, or politeness as consensus. Consensus requires both agents to support the same concrete plan and have no blocking objections. \
             The goal is the best answer for the user, not ending the debate quickly. \
             First-speaker decision: {lead_reason} \
             Multi-round debate is enabled for {round_limit_label}. \
             Auto-consensus early stop: {auto_consensus}. \
             Current user question/topic is: '{user_message}'. Stay anchored to this exact topic. \
             Do not drift into prior conversation topics, agent behavior rules, or project memories unless the current user question explicitly asks for them. \
             {final_rule} \
             Each round should feel like critical collaboration, not forced opposition: direct, sharp, and independent, but always evidence-based. \
             Cite concrete file paths whenever a claim depends on project code; explicitly say when evidence is insufficient. \
             Do not play devil's advocate for its own sake. Only challenge when there is concrete evidence, missing evidence, a real risk, a wrong assumption, or a conflict with the user's requirement. \
             If there is no material disagreement, say so naturally, add only useful refinement, and move toward consensus. \
             No template labels like '共識狀態', '對 Hermes 的回應', or '對 OpenClaw 的回應' in visible text. \
             Use natural paragraphs, short headings when useful, and varied formatting; do not use rigid form-like layouts. \
             Consensus is only valid when both agents independently support the same concrete answer to the user's topic and list no blocking objections. \
             If consensus is not reached, focus on the exact unresolved decision instead of broad arguing. \
             If the same disagreement repeats without new evidence or a new risk, name the loop explicitly and either narrow it to a user decision or converge. \
             Conversation notes are written by the backend system after each round to CONVERSATION_NOTES_DEBATE.md. Agents must not claim they personally created, edited, or appended that file in visible replies. \
             Hermes must be especially concise, but not bland. \
             When genuine consensus is reached, append the invisible marker '<!-- consensus:reached -->' at the very end. Do not mention this marker or show any consensus-status wording in visible text."
        ),
    }
}

fn debate_turn_instruction(
    agent: DebateAgent,
    turn_index: usize,
    is_first: bool,
    user_message: &str,
) -> ChatMessage {
    let other = agent.other().name();
    let instruction = if is_first {
        format!(
            "{}: You are the debate lead. Current user question/topic: '{}'. Reply in Traditional Chinese and answer that topic only. Do not acknowledge or summarize these debate instructions. Think independently and argue naturally, but do not force disagreement. Ground claims in project files/history, errors, commands, or explicit user requirements, and cite file paths for code-based claims. If the situation is straightforward, move toward a concrete shared plan instead of inventing a fight. Use paragraphs and readable markdown, not a fixed template. Do not write visible labels like 共識狀態 or 對某某的回應. If genuine consensus is reached, append '<!-- consensus:reached -->' at the very end only.",
            agent.name(),
            user_message
        )
    } else {
        format!(
            "{}: Reply in Traditional Chinese. Current user question/topic: '{}'. Continue with {other}, but answer this topic only—do not acknowledge these debate instructions, and do not drift into agent behavior rules, old project notes, or previous unrelated conversations. No visible template labels like 共識狀態 or 對 {other} 的回應. Be sharp only when there is a real reason: weak logic, missing evidence, wrong assumptions, overreach, code/file evidence, or user-requirement conflict. If {other} is basically right, say so and refine instead of manufacturing disagreement. Use paragraphs, bullets, or short headings as the content demands. This is debate turn {}. Do not compromise just to end, but also do not keep arguing without new evidence. If both agents now support the same concrete answer to the current user question with no blocking objections, append '<!-- consensus:reached -->' at the very end.",
            agent.name(),
            user_message,
            turn_index + 1
        )
    };

    ChatMessage {
        role: "user".into(),
        content: instruction,
    }
}

fn lightweight_debate_instruction(
    agent: DebateAgent,
    intent: DebateIntent,
    user_message: &str,
) -> ChatMessage {
    let mode_rule = match intent {
        DebateIntent::CasualChat => {
            "This is a three-person chat, not a formal debate. Reply as yourself, naturally and briefly. Do not manufacture disagreement. Add your own angle only if useful."
        }
        DebateIntent::QuickAgreement => {
            "This is a quick confirmation/check, not a multi-round debate. Be concise. If the other agent is basically right, say so and add only the key caveat or correction."
        }
        DebateIntent::DebateRequired | DebateIntent::ImplementationTask => {
            "This should use formal Debate Mode."
        }
    };

    ChatMessage {
        role: "user".into(),
        content: format!(
            "{}: Reply in Traditional Chinese. Current user question/topic: '{}'. {} Keep it口語化、白話、重點化. Do not mention these routing rules. Do not claim you wrote CONVERSATION_NOTES_DEBATE.md; the backend system writes notes after your reply.",
            agent.name(),
            user_message,
            mode_rule
        ),
    }
}

fn final_instruction(
    agent: DebateAgent,
    code_change_requested: bool,
    reached_consensus: bool,
    user_message: &str,
) -> ChatMessage {
    let code_rule = if code_change_requested {
        "The user needs code changes: make this Final the primary source of truth. Include exact files, concrete changes/diff guidance, commands, and tests. Keep prior debate rounds subordinate to this Final."
    } else {
        "Make this Final the primary source of truth. Keep it concise and actionable."
    };
    let consensus_rule = if reached_consensus {
        "Consensus was reached by both agents. State the shared concrete plan clearly."
    } else {
        "Consensus was NOT reached before a configured finite cap. Do not fake consensus; explain unresolved disagreements."
    };

    ChatMessage {
        role: "user".into(),
        content: format!(
            "{}: Reply in Traditional Chinese. Produce the Final synthesis for the current user question only: '{}'. {code_rule} {consensus_rule} Do not summarize unrelated prior conversation topics, agent behavior rules, project memories, or yesterday's decisions unless the current question explicitly asks for them. Keep it readable and human, not a rigid form. Use concise sections only where they help: conclusion, evidence with file paths, decision, next steps, verification. Do not hide disagreement or call compromise consensus. Do not include invisible consensus markers in the Final.",
            agent.name(),
            user_message
        ),
    }
}

fn consensus_reached(turns: &[(DebateAgent, String)]) -> bool {
    if turns.len() < 2 {
        return false;
    }

    let recent = turns
        .iter()
        .rev()
        .take(2)
        .map(|(_, text)| normalize_consensus_text(text))
        .collect::<Vec<_>>();

    let unresolved_markers = [
        "disagree",
        "不同意",
        "反對",
        "尚未",
        "未解",
        " unresolved",
        "however",
        "但是",
        "不過",
        "仍然",
        "concern",
        "疑慮",
        "風險仍",
        "不能接受",
        "需要使用者決策",
        "交給 user",
        "交給使用者",
        "缺少證據",
        "資訊不足",
    ];

    if recent.iter().any(|text| {
        unresolved_markers
            .iter()
            .any(|marker| text.contains(marker))
    }) {
        return false;
    }

    recent.iter().all(|text| has_consensus_signal(text))
}

fn normalize_consensus_text(text: &str) -> String {
    text.to_lowercase()
        .replace("沒有新的反對點", "no_objection")
        .replace("沒有新反對點", "no_objection")
        .replace("沒有新的實質反對", "no_objection")
        .replace("沒有實質反對", "no_objection")
        .replace("沒有阻礙性反對", "no_objection")
        .replace("沒有新的阻礙性反對", "no_objection")
        .replace("沒有新的阻塞", "no_objection")
        .replace("沒有反對", "no_objection")
        .replace("無反對", "no_objection")
        .replace("我這邊沒有新的反對點", "no_objection")
        .replace("我沒有新的反對點", "no_objection")
        .replace("我同意", "i_agree")
        .replace("方向是對的", "i_agree")
        .replace("補得對", "i_agree")
        .replace("沒有 blocking objection", "no_objection")
        .replace("no blocking objection", "no_objection")
        .replace("沒有阻塞異議", "no_objection")
        .replace("沒有阻塞分歧", "no_objection")
        .replace("沒有阻塞", "no_objection")
        .replace("無阻塞", "no_objection")
}

fn has_consensus_signal(text: &str) -> bool {
    [
        "<!-- consensus:reached -->",
        "no_objection",
        "沒有需要硬辯",
        "不需要硬辯",
        "不硬製造分歧",
        "沒有實質反對",
        "沒有新的反對點",
        "沒有新的實質反對",
        "沒有阻礙性反對",
        "i_agree",
        "我同意",
        "沒有 material disagreement",
        "基本正確",
        "方向沒問題",
        "方向是對的",
        "補得對",
        "我支持",
        "同一個具體方向",
        "同一個具體方案",
        "可執行的共同",
        "收斂後",
        "結論很簡單",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

fn code_change_requested(user_message: &str) -> bool {
    let text = user_message.to_lowercase();
    [
        "改程式",
        "調整程式",
        "修改程式",
        "修 bug",
        "debug",
        "實作",
        "開發",
        "寫 code",
        "code",
        "diff",
        "patch",
        "修正",
        "新增",
        "重構",
        "build",
        "lint",
        "cargo",
        "npm",
    ]
    .iter()
    .any(|kw| text.contains(kw))
}

async fn run_agent(
    openclaw: &OpenClawClient,
    hermes: &HermesClient,
    agent: DebateAgent,
    ctx: Vec<ChatMessage>,
) -> Result<String> {
    match agent {
        DebateAgent::OpenClaw => openclaw.chat(ctx).await,
        DebateAgent::Hermes => hermes.chat(ctx).await,
    }
}

fn chat_with_turns(
    base: &[ChatMessage],
    turns: &[(DebateAgent, String)],
    instruction: ChatMessage,
) -> Vec<ChatMessage> {
    let mut ctx = base.to_vec();
    for (agent, content) in turns {
        ctx.push(ChatMessage {
            role: "assistant".into(),
            content: format!("[{}]: {}", agent.name(), content),
        });
    }
    ctx.push(instruction);
    ctx
}

fn append_debate_note(
    project_root: Option<&str>,
    agent: DebateAgent,
    phase_label: &str,
    user_message: &str,
    reply: &str,
) {
    let Some(root) = project_root else {
        return;
    };
    let path = Path::new(root).join(DEBATE_NOTES_FILE);
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };

    let highlights = extract_note_highlights(reply);
    let keywords = extract_note_keywords(user_message, reply);
    let entry = format!(
        "\n---\n\n## {} · {} · {}\n\n- 使用者問題：{}\n- 對話重點：\n{}\n- 關鍵字：{}\n- 紀錄規則：此筆由系統依該 Agent 本輪回覆自動追加；只往下新增，不覆蓋舊內容。\n",
        Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        agent.name(),
        phase_label,
        sanitize_note_line(user_message, 240),
        highlights,
        keywords.join("、")
    );
    let _ = file.write_all(entry.as_bytes());
}

fn extract_note_highlights(reply: &str) -> String {
    let mut lines = reply
        .replace("\r", "")
        .lines()
        .map(|line| line.trim().trim_start_matches(['-', '*', '#', ' ']).trim())
        .filter(|line| !line.is_empty())
        .filter(|line| !line.contains("<!-- consensus:reached -->"))
        .take(5)
        .map(|line| format!("  - {}", sanitize_note_line(line, 180)))
        .collect::<Vec<_>>();

    if lines.is_empty() {
        lines.push("  - 本輪沒有可摘錄的有效文字。".into());
    }
    lines.join("\n")
}

fn extract_note_keywords(user_message: &str, reply: &str) -> Vec<String> {
    let text = format!("{}\n{}", user_message, reply).to_lowercase();
    let candidates = [
        "OpenClaw",
        "Hermes",
        "Debate",
        "Final",
        "共識",
        "分歧",
        "阻塞",
        "重複",
        "濃縮",
        "CONVERSATION_NOTES_DEBATE.md",
        "Git",
        "Branch",
        "Identity",
        "Repository",
        "Token",
        "驗證",
        "專案",
        "記憶",
        "對話紀錄",
        "關鍵字",
        "重點",
        "風險",
        "實作",
        "測試",
        "cargo",
        "npm",
        "lint",
        "build",
        "streaming",
        "scroll",
        "WebSocket",
    ];

    let mut keywords = candidates
        .iter()
        .filter(|keyword| text.contains(&keyword.to_lowercase()))
        .map(|keyword| (*keyword).to_string())
        .take(10)
        .collect::<Vec<_>>();

    if keywords.is_empty() {
        keywords.push("一般對話".into());
    }
    keywords
}

fn sanitize_note_line(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut output = compact.chars().take(max_chars).collect::<String>();
    if compact.chars().count() > max_chars {
        output.push('…');
    }
    output
}

/// One round of the debate (or the final synthesis), packaged with its
/// chat context and the agent that should answer it.
struct DebateRound {
    agent: DebateAgent,
    context: Vec<ChatMessage>,
    /// 1-based round number for round phase; `None` for final synthesis.
    round_number: Option<usize>,
}

/// Shared driver for Debate Mode used by both the REST (run_agent_turn)
/// and WebSocket (run_agent_stream) paths. Owns the running list of turns
/// and decides when to stop (max rounds or consensus).
struct DebateRunner {
    base: Vec<ChatMessage>,
    user_message: String,
    code_change: bool,
    round_limit: usize,
    auto_consensus: bool,
    lead: DebateAgent,
    project_root: Option<String>,
    turns: Vec<(DebateAgent, String)>,
    current: DebateAgent,
    turn_index: usize,
    reached_consensus: bool,
}

impl DebateRunner {
    fn new(
        config: &Config,
        history: &[Message],
        user_message: &str,
        project_root: Option<String>,
        chat_with_user_msg: Vec<ChatMessage>,
    ) -> Self {
        let lead = choose_debate_lead(history, user_message);
        let round_limit = debate_round_limit(config);
        let code_change = code_change_requested(user_message);
        let mut base = vec![debate_instruction(
            lead,
            round_limit,
            config.debate_auto_consensus,
            code_change,
            user_message,
        )];
        base.extend(chat_with_user_msg);
        Self {
            base,
            user_message: user_message.to_string(),
            code_change,
            round_limit,
            auto_consensus: config.debate_auto_consensus,
            lead,
            project_root,
            turns: Vec::new(),
            current: lead,
            turn_index: 0,
            reached_consensus: false,
        }
    }

    fn next_round(&self) -> Option<DebateRound> {
        if self.reached_consensus || self.turn_index >= self.round_limit {
            return None;
        }
        let ctx = chat_with_turns(
            &self.base,
            &self.turns,
            debate_turn_instruction(
                self.current,
                self.turn_index,
                self.turn_index == 0,
                &self.user_message,
            ),
        );
        Some(DebateRound {
            agent: self.current,
            context: ctx,
            round_number: Some(self.turn_index + 1),
        })
    }

    fn record_turn(&mut self, reply: String) {
        let phase_label = format!("Round {}", self.turn_index + 1);
        append_debate_note(
            self.project_root.as_deref(),
            self.current,
            &phase_label,
            &self.user_message,
            &reply,
        );
        self.turns.push((self.current, reply));
        if self.auto_consensus && consensus_reached(&self.turns) {
            self.reached_consensus = true;
            return;
        }
        self.current = self.current.other();
        self.turn_index += 1;
    }

    fn final_round(&self) -> DebateRound {
        let ctx = chat_with_turns(
            &self.base,
            &self.turns,
            final_instruction(
                self.lead,
                self.code_change,
                self.reached_consensus,
                &self.user_message,
            ),
        );
        DebateRound {
            agent: self.lead,
            context: ctx,
            round_number: None,
        }
    }
}

pub async fn run_agent_turn(
    config: &Arc<Config>,
    project: &ProjectScope,
    history: &[Message],
    project_summary: Option<&str>,
    user_message: &str,
    mode: AgentMode,
) -> Result<Vec<(String, String, Option<String>)>> {
    let openclaw = OpenClawClient::new(config);
    let hermes = HermesClient::new(config);

    let mut chat = messages_to_chat(project, history, project_summary);
    chat.push(ChatMessage {
        role: "user".into(),
        content: user_message.into(),
    });

    let mut results = vec![];
    match mode {
        AgentMode::OpenClawOnly => {
            let reply = run_agent(&openclaw, &hermes, DebateAgent::OpenClaw, chat).await?;
            results.push(("openclaw".into(), reply, Some("OpenClaw".into())));
        }
        AgentMode::HermesOnly => {
            let reply = run_agent(&openclaw, &hermes, DebateAgent::Hermes, chat).await?;
            results.push(("hermes".into(), reply, Some("Hermes".into())));
        }
        AgentMode::Custom(profile) => {
            let display_name = profile.name.clone();
            let reply = GenericAgentClient::new(profile).chat(chat).await?;
            results.push(("openclaw".into(), reply.content, Some(display_name)));
        }
        AgentMode::CustomDebate(participants) => {
            let mut turns: Vec<(String, String)> = Vec::new();
            for participant in participants.iter().take(4).cloned() {
                let mut ctx = chat.clone();
                for (name, reply) in &turns {
                    ctx.push(ChatMessage {
                        role: "assistant".into(),
                        content: format!("[{name}]: {reply}"),
                    });
                }
                let display_name = participant.display_name();
                ctx.push(ChatMessage {
                    role: "user".into(),
                    content: format!(
                        "{}: 這是多 Agent Debate。請針對目前使用者問題提出獨立觀點，必要時挑戰前面 Agent，引用檔案證據，不要硬製造分歧。",
                        display_name
                    ),
                });
                // Built-in participants dispatch through the same OpenClawClient /
                // HermesClient that the original Debate path uses, so their
                // outputs match what users already expect. Custom profiles fall
                // through to GenericAgentClient.
                let content = match participant {
                    DebateParticipant::OpenClaw => openclaw.chat(ctx).await?,
                    DebateParticipant::Hermes => hermes.chat(ctx).await?,
                    DebateParticipant::Custom(profile) => {
                        GenericAgentClient::new(profile).chat(ctx).await?.content
                    }
                };
                turns.push((display_name.clone(), content.clone()));
                // We bucket every participant under the "openclaw" role so the
                // messages table CHECK constraint stays happy — the actual
                // display attribution lives in agent_name. (See mig 0009
                // for the role enum.)
                results.push(("openclaw".into(), content, Some(display_name)));
            }
        }
        AgentMode::Debate => {
            let intent = classify_debate_intent(user_message);
            match intent {
                DebateIntent::CasualChat | DebateIntent::QuickAgreement => {
                    let lead = choose_debate_lead(history, user_message);
                    let mut turns: Vec<(DebateAgent, String)> = Vec::new();
                    for agent in [lead, lead.other()] {
                        let ctx = chat_with_turns(
                            &chat,
                            &turns,
                            lightweight_debate_instruction(agent, intent, user_message),
                        );
                        let reply = run_agent(&openclaw, &hermes, agent, ctx).await?;
                        append_debate_note(
                            project.root.as_deref(),
                            agent,
                            match intent {
                                DebateIntent::CasualChat => "Chat",
                                DebateIntent::QuickAgreement => "Quick Check",
                                _ => "Round",
                            },
                            user_message,
                            &reply,
                        );
                        turns.push((agent, reply.clone()));
                        results.push((agent.role().into(), reply, Some(agent.name().into())));
                    }
                }
                DebateIntent::DebateRequired | DebateIntent::ImplementationTask => {
                    let mut runner = DebateRunner::new(
                        config,
                        history,
                        user_message,
                        project.root.clone(),
                        chat.clone(),
                    );
                    while let Some(round) = runner.next_round() {
                        let reply =
                            run_agent(&openclaw, &hermes, round.agent, round.context).await?;
                        results.push((
                            round.agent.role().into(),
                            reply.clone(),
                            Some(round.agent.name().into()),
                        ));
                        runner.record_turn(reply);
                    }
                    let final_round = runner.final_round();
                    let final_reply =
                        run_agent(&openclaw, &hermes, final_round.agent, final_round.context)
                            .await?;
                    append_debate_note(
                        project.root.as_deref(),
                        final_round.agent,
                        "Final",
                        user_message,
                        &final_reply,
                    );
                    results.push((
                        final_round.agent.role().into(),
                        final_reply,
                        Some(final_round.agent.name().into()),
                    ));
                }
            }
        }
    }
    Ok(results)
}

pub fn run_agent_stream(
    config: &Arc<Config>,
    project: &ProjectScope,
    history: &[Message],
    project_summary: Option<String>,
    user_message: &str,
    mode: AgentMode,
) -> Pin<Box<dyn Stream<Item = ServerEvent> + Send>> {
    let config = config.clone();
    let project = project.clone();
    let chunk_timeout = stream_chunk_timeout(&config);
    let history_owned: Vec<Message> = history.to_vec();
    let current_topic = user_message.to_string();
    let mut chat = messages_to_chat(&project, history, project_summary.as_deref());
    chat.push(ChatMessage {
        role: "user".into(),
        content: current_topic.clone(),
    });

    Box::pin(async_stream::stream! {
        let openclaw = OpenClawClient::new(&config);
        let hermes = HermesClient::new(&config);

        match mode {
            AgentMode::OpenClawOnly => {
                let input_tokens = Some(estimate_chat_tokens(&chat));
                yield ServerEvent::Status {
                    agent: "OpenClaw".into(),
                    message: "OpenClaw 正在整理問題與專案脈絡...".into(),
                    round: None,
                    phase: Some("thinking".into()),
                    input_tokens,
                };
                let mut stream = batch_chunks(openclaw.chat_stream(chat));
                let mut response_metadata = None;
                loop {
                    match tokio::time::timeout(chunk_timeout, stream.next()).await {
                        Ok(Some(AgentStreamEvent::Content(chunk))) => yield ServerEvent::Chunk {
                            agent: "OpenClaw".into(), content: chunk, round: None, phase: None,
                        },
                        Ok(Some(AgentStreamEvent::Metadata(metadata))) => response_metadata = Some(metadata),
                        Ok(None) => break,
                        Err(_) => {
                            yield ServerEvent::Error { message: "OpenClaw stream timed out".into() };
                            return;
                        }
                    }
                }
                yield done_event("OpenClaw".into(), None, None, response_metadata);
            }
            AgentMode::HermesOnly => {
                let input_tokens = Some(estimate_chat_tokens(&chat));
                yield ServerEvent::Status {
                    agent: "Hermes".into(),
                    message: "Hermes 正在整理問題與專案脈絡...".into(),
                    round: None,
                    phase: Some("thinking".into()),
                    input_tokens,
                };
                let mut stream = batch_chunks(hermes.chat_stream(chat));
                let mut response_metadata = None;
                loop {
                    match tokio::time::timeout(chunk_timeout, stream.next()).await {
                        Ok(Some(AgentStreamEvent::Content(chunk))) => yield ServerEvent::Chunk {
                            agent: "Hermes".into(), content: chunk, round: None, phase: None,
                        },
                        Ok(Some(AgentStreamEvent::Metadata(metadata))) => response_metadata = Some(metadata),
                        Ok(None) => break,
                        Err(_) => {
                            yield ServerEvent::Error { message: "Hermes stream timed out".into() };
                            return;
                        }
                    }
                }
                yield done_event("Hermes".into(), None, None, response_metadata);
            }
            AgentMode::Custom(profile) => {
                let agent_name = profile.name.clone();
                let input_tokens = Some(estimate_chat_tokens(&chat));
                yield ServerEvent::Status {
                    agent: agent_name.clone(),
                    message: format!("{} 正在整理問題與專案脈絡...", agent_name),
                    round: None,
                    phase: Some("thinking".into()),
                    input_tokens,
                };
                let custom = GenericAgentClient::new(profile);
                let mut stream = batch_chunks(custom.chat_stream(chat));
                let mut response_metadata = None;
                loop {
                    match tokio::time::timeout(chunk_timeout, stream.next()).await {
                        Ok(Some(AgentStreamEvent::Content(chunk))) => yield ServerEvent::Chunk {
                            agent: agent_name.clone(), content: chunk, round: None, phase: None,
                        },
                        Ok(Some(AgentStreamEvent::Metadata(metadata))) => response_metadata = Some(metadata),
                        Ok(None) => break,
                        Err(_) => {
                            yield ServerEvent::Error { message: format!("{} stream timed out", agent_name) };
                            return;
                        }
                    }
                }
                yield done_event(agent_name, None, None, response_metadata);
            }
            AgentMode::CustomDebate(participants) => {
                // Streaming dispatcher per participant. Built-in OpenClaw /
                // Hermes use their dedicated clients (matching the legacy
                // Debate behaviour); user-defined profiles fall through to
                // GenericAgentClient. Same Stream<Item = AgentStreamEvent>
                // signature on all three so the rest of the loop is generic.
                let participants = participants.into_iter().take(4).collect::<Vec<_>>();
                let stream_for = |p: DebateParticipant, ctx: Vec<ChatMessage>|
                    -> Pin<Box<dyn Stream<Item = AgentStreamEvent> + Send>>
                {
                    match p {
                        DebateParticipant::OpenClaw => openclaw.chat_stream(ctx),
                        DebateParticipant::Hermes => hermes.chat_stream(ctx),
                        DebateParticipant::Custom(profile) => {
                            GenericAgentClient::new(profile).chat_stream(ctx)
                        }
                    }
                };

                let mut turns: Vec<(String, String)> = Vec::new();
                for (idx, participant) in participants.iter().cloned().enumerate() {
                    let agent_name = participant.display_name();
                    let mut ctx = chat.clone();
                    for (name, reply) in &turns {
                        ctx.push(ChatMessage {
                            role: "assistant".into(),
                            content: format!("[{name}]: {reply}"),
                        });
                    }
                    ctx.push(ChatMessage {
                        role: "user".into(),
                        content: format!(
                            "{}: 這是多 Agent Debate 第 {} 位發言。請針對目前使用者問題提出獨立觀點，必要時挑戰前面 Agent，引用檔案證據，不要硬製造分歧。",
                            agent_name,
                            idx + 1
                        ),
                    });
                    let input_tokens = Some(estimate_chat_tokens(&ctx));
                    yield ServerEvent::Status {
                        agent: agent_name.clone(),
                        message: format!("{} 正在分析前面觀點並整理回覆...", agent_name),
                        round: Some(idx + 1),
                        phase: Some("round".into()),
                        input_tokens,
                    };
                    let mut buffer = String::new();
                    let mut stream = batch_chunks(stream_for(participant, ctx));
                    let mut response_metadata = None;
                    loop {
                        match tokio::time::timeout(chunk_timeout, stream.next()).await {
                            Ok(Some(AgentStreamEvent::Content(chunk))) => {
                                buffer.push_str(&chunk);
                                yield ServerEvent::Chunk {
                                    agent: agent_name.clone(),
                                    content: chunk,
                                    round: Some(idx + 1),
                                    phase: Some("round".into()),
                                };
                            }
                            Ok(Some(AgentStreamEvent::Metadata(metadata))) => response_metadata = Some(metadata),
                            Ok(None) => break,
                            Err(_) => {
                                if buffer.trim().is_empty() {
                                    yield ServerEvent::Error { message: format!("{} stream timed out", agent_name) };
                                    return;
                                }
                                break;
                            }
                        }
                    }
                    yield done_event(agent_name.clone(), Some(idx + 1), Some("round".into()), response_metadata);
                    turns.push((agent_name, buffer));
                }

                // Synthesis: first participant integrates the rest.
                if let Some(participant) = participants.first().cloned() {
                    let agent_name = participant.display_name();
                    let mut ctx = chat.clone();
                    for (name, reply) in &turns {
                        ctx.push(ChatMessage {
                            role: "assistant".into(),
                            content: format!("[{name}]: {reply}"),
                        });
                    }
                    ctx.push(ChatMessage {
                        role: "user".into(),
                        content: "請綜合所有 Agent 的觀點，產出最終結論：共識、分歧、建議方案、風險、下一步。不要假裝已修改程式。".into(),
                    });
                    let input_tokens = Some(estimate_chat_tokens(&ctx));
                    yield ServerEvent::Status {
                        agent: agent_name.clone(),
                        message: format!("{} 正在彙整多 Agent 最終結論...", agent_name),
                        round: None,
                        phase: Some("final".into()),
                        input_tokens,
                    };
                    let mut stream = batch_chunks(stream_for(participant, ctx));
                    let mut response_metadata = None;
                    loop {
                        match tokio::time::timeout(chunk_timeout, stream.next()).await {
                            Ok(Some(AgentStreamEvent::Content(chunk))) => yield ServerEvent::Chunk {
                                agent: agent_name.clone(), content: chunk, round: None, phase: Some("final".into()),
                            },
                            Ok(Some(AgentStreamEvent::Metadata(metadata))) => response_metadata = Some(metadata),
                            Ok(None) => break,
                            Err(_) => {
                                yield ServerEvent::Error { message: format!("{} stream timed out", agent_name) };
                                return;
                            }
                        }
                    }
                    yield done_event(agent_name, None, Some("final".into()), response_metadata);
                }
            }
            AgentMode::Debate => {
                let intent = classify_debate_intent(&current_topic);
                if matches!(intent, DebateIntent::CasualChat | DebateIntent::QuickAgreement) {
                    let lead = choose_debate_lead(&history_owned, &current_topic);
                    let mut turns: Vec<(DebateAgent, String)> = Vec::new();
                    for agent in [lead, lead.other()] {
                        let agent_name = agent.name().to_string();
                        let phase = match intent {
                            DebateIntent::CasualChat => "chat",
                            DebateIntent::QuickAgreement => "quick_check",
                            _ => "round",
                        };
                        let ctx = chat_with_turns(
                            &chat,
                            &turns,
                            lightweight_debate_instruction(agent, intent, &current_topic),
                        );
                        let input_tokens = Some(estimate_chat_tokens(&ctx));
                        yield ServerEvent::Status {
                            agent: agent_name.clone(),
                            message: format!("{} 正在整理回覆...", agent_name),
                            round: None,
                            phase: Some(phase.into()),
                            input_tokens,
                        };
                        let mut buffer = String::new();
                        let mut stream = batch_chunks(match agent {
                            DebateAgent::OpenClaw => openclaw.chat_stream(ctx),
                            DebateAgent::Hermes => hermes.chat_stream(ctx),
                        });
                        let mut response_metadata = None;
                        loop {
                            match tokio::time::timeout(chunk_timeout, stream.next()).await {
                                Ok(Some(AgentStreamEvent::Content(chunk))) => {
                                    buffer.push_str(&chunk);
                                    yield ServerEvent::Chunk {
                                        agent: agent_name.clone(),
                                        content: chunk,
                                        round: None,
                                        phase: Some(phase.into()),
                                    };
                                }
                                Ok(Some(AgentStreamEvent::Metadata(metadata))) => response_metadata = Some(metadata),
                                Ok(None) => break,
                                Err(_) => {
                                    yield ServerEvent::Error {
                                        message: format!("{} stream timed out", agent_name),
                                    };
                                    return;
                                }
                            }
                        }
                        yield done_event(agent_name, None, Some(phase.into()), response_metadata);
                        append_debate_note(
                            project.root.as_deref(),
                            agent,
                            match intent {
                                DebateIntent::CasualChat => "Chat",
                                DebateIntent::QuickAgreement => "Quick Check",
                                _ => "Round",
                            },
                            &current_topic,
                            &buffer,
                        );
                        turns.push((agent, buffer));
                    }
                    return;
                }

                let mut runner = DebateRunner::new(&config, &history_owned, &current_topic, project.root.clone(), chat);
                while let Some(round) = runner.next_round() {
                    let agent_name = round.agent.name().to_string();
                    let round_num = round.round_number;
                    let input_tokens = Some(estimate_chat_tokens(&round.context));
                    yield ServerEvent::Status {
                        agent: agent_name.clone(),
                        message: format!(
                            "{} · Round {} 正在分析、檢查反例與整理觀點...",
                            agent_name,
                            round_num.unwrap_or(0)
                        ),
                        round: round_num,
                        phase: Some("round".into()),
                        input_tokens,
                    };
                    let mut buffer = String::new();
                    let mut stream = batch_chunks(match round.agent {
                        DebateAgent::OpenClaw => openclaw.chat_stream(round.context),
                        DebateAgent::Hermes => hermes.chat_stream(round.context),
                    });
                    let mut timed_out = false;
                    let mut response_metadata = None;
                    loop {
                        match tokio::time::timeout(chunk_timeout, stream.next()).await {
                            Ok(Some(AgentStreamEvent::Content(chunk))) => {
                                buffer.push_str(&chunk);
                                yield ServerEvent::Chunk {
                                    agent: agent_name.clone(),
                                    content: chunk,
                                    round: round_num,
                                    phase: Some("round".into()),
                                };
                            }
                            Ok(Some(AgentStreamEvent::Metadata(metadata))) => response_metadata = Some(metadata),
                            Ok(None) => break,
                            Err(_) => {
                                timed_out = true;
                                if buffer.trim().is_empty() {
                                    yield ServerEvent::Error {
                                        message: format!(
                                            "{} stream timed out before producing content at round {}",
                                            agent_name,
                                            round_num.unwrap_or(0)
                                        ),
                                    };
                                }
                                break;
                            }
                        }
                    }
                    if timed_out && buffer.trim().is_empty() {
                        return;
                    }
                    yield done_event(agent_name, round_num, Some("round".into()), response_metadata);
                    runner.record_turn(buffer);
                    if timed_out {
                        break;
                    }
                }

                let final_round = runner.final_round();
                let final_agent_name = final_round.agent.name().to_string();
                let final_agent = final_round.agent;
                let input_tokens = Some(estimate_chat_tokens(&final_round.context));
                yield ServerEvent::Status {
                    agent: final_agent_name.clone(),
                    message: format!("{} · Final 正在彙整最終結論...", final_agent_name),
                    round: None,
                    phase: Some("final".into()),
                    input_tokens,
                };
                let mut final_buffer = String::new();
                let mut stream = batch_chunks(match final_round.agent {
                    DebateAgent::OpenClaw => openclaw.chat_stream(final_round.context),
                    DebateAgent::Hermes => hermes.chat_stream(final_round.context),
                });
                let mut response_metadata = None;
                loop {
                    match tokio::time::timeout(chunk_timeout, stream.next()).await {
                        Ok(Some(AgentStreamEvent::Content(chunk))) => {
                            final_buffer.push_str(&chunk);
                            yield ServerEvent::Chunk {
                                agent: final_agent_name.clone(),
                                content: chunk,
                                round: None,
                                phase: Some("final".into()),
                            }
                        },
                        Ok(Some(AgentStreamEvent::Metadata(metadata))) => response_metadata = Some(metadata),
                        Ok(None) => break,
                        Err(_) => {
                            if final_buffer.trim().is_empty() {
                                yield ServerEvent::Error {
                                    message: format!(
                                        "{} final synthesis stream timed out before producing content",
                                        final_agent_name
                                    ),
                                };
                                return;
                            }
                            break;
                        }
                    }
                }
                if final_buffer.trim().is_empty() {
                    return;
                }
                yield done_event(final_agent_name, None, Some("final".into()), response_metadata);
                append_debate_note(
                    project.root.as_deref(),
                    final_agent,
                    "Final",
                    &current_topic,
                    &final_buffer,
                );
            }
        }
    })
}
