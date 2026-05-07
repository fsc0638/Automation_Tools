use anyhow::Result;
use futures_util::{Stream, StreamExt};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    agents::{hermes::HermesClient, openclaw::{ChatMessage, OpenClawClient}},
    config::Config,
    db::models::{Message, Project},
};

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type")]
pub enum ServerEvent {
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
    },
    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Debug, Clone)]
pub enum AgentMode {
    HermesOnly,
    OpenClawOnly,
    Debate,
}

#[derive(Debug, Clone)]
pub struct ProjectScope {
    pub id: Uuid,
    pub name: String,
    pub source_type: String,
    pub root: Option<String>,
    pub file_snapshot: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DebateAgent {
    OpenClaw,
    Hermes,
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
    let root = if project.source_type == "git" {
        project.local_path.clone().unwrap_or_else(|| project.source_path.clone())
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
    }
}

fn absolutize_path(path: &str) -> Option<String> {
    let path = PathBuf::from(path);
    let candidate = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    candidate.canonicalize().ok().map(|p| p.to_string_lossy().to_string())
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
            let rel = file.strip_prefix(root_path).unwrap_or(&file).to_string_lossy().replace('\\', "/");
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
    let Ok(entries) = std::fs::read_dir(dir) else { return; };
    let mut entries = entries.filter_map(|e| e.ok()).collect::<Vec<_>>();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        if out.len() >= 180 { break; }
        let name = entry.file_name().to_string_lossy().to_string();
        if should_ignore(&name) { continue; }
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        out.push(format!("{}{}{}", "  ".repeat(depth), rel, if is_dir { "/" } else { "" }));
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
        "README.md", "readme.md", "package.json", "Cargo.toml", "pyproject.toml",
        "requirements.txt", "go.mod", "pom.xml", "build.gradle", "docker-compose.yml",
        "Dockerfile", "src/main.rs", "src/main.ts", "src/main.tsx", "src/index.ts",
        "src/index.tsx", "src/App.tsx", "main.py", "app.py",
    ];
    candidates.iter().map(|p| root.join(p)).filter(|p| p.is_file()).collect()
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
             If the file snapshot below is missing or insufficient, explicitly say the project files are unavailable/insufficient. \
             Do not invent architecture from the project name.\n\n{}",
            project.id, project.name, project.source_type, root,
            project.file_snapshot.as_deref().unwrap_or("[No project file snapshot available]")
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

fn messages_to_chat(project: &ProjectScope, history: &[Message], project_summary: Option<&str>) -> Vec<ChatMessage> {
    let mut messages = vec![project_scope_message(project)];
    if let Some(summary) = project_summary.filter(|s| !s.trim().is_empty()) {
        messages.push(project_summary_message(summary));
    }
    messages.extend(history.iter().map(|m| {
        let role = if m.role == "user" { "user" } else { "assistant" };
        let content = match m.role.as_str() {
            "openclaw" => format!("[OpenClaw]: {}", m.content),
            "hermes" => format!("[Hermes]: {}", m.content),
            "system" => format!("[System]: {}", m.content),
            _ => m.content.clone(),
        };

        ChatMessage {
            role: role.into(),
            content,
        }
    }));
    messages
}

fn choose_debate_lead(history: &[Message], user_message: &str) -> DebateAgent {
    let text = format!(
        "{}\n{}",
        history.iter().rev().take(8).map(|m| m.content.as_str()).collect::<Vec<_>>().join("\n"),
        user_message
    ).to_lowercase();

    let openclaw_keywords = [
        "架構", "設計", "規劃", "計畫", "技術選型", "大型", "複雜", "系統", "重構",
        "race", "deadlock", "concurrency", "async", "間歇", "難題", "根因", "root cause",
        "security", "安全", "migration", "資料模型", "跨", "影響範圍", "邊界", "scal",
        "performance", "效能", "memory", "debug 難", "難 bug",
    ];
    let hermes_keywords = [
        "寫", "新增", "修改", "調整", "修正", "一般", "簡單", "快速", "日常", "樣式",
        "ui", "copy", "文字", "按鈕", "表單", "lint", "format", "小改", "直接修",
    ];

    let openclaw_score = openclaw_keywords.iter().filter(|kw| text.contains(**kw)).count();
    let hermes_score = hermes_keywords.iter().filter(|kw| text.contains(**kw)).count();

    if openclaw_score > hermes_score || text.len() > 700 {
        DebateAgent::OpenClaw
    } else {
        DebateAgent::Hermes
    }
}

fn debate_round_limit_label(max_rounds: usize) -> String {
    format!("up to {max_rounds} agent turns, then stop and return unresolved disagreements to the user")
}

fn debate_round_limit(config: &Config) -> usize {
    // 0 used to mean unlimited, but that created unstoppable debate loops.
    // Treat 0 as the safe default instead.
    if config.debate_max_rounds == 0 {
        12
    } else {
        config.debate_max_rounds.clamp(2, 20)
    }
}

fn debate_instruction(lead: DebateAgent, round_limit: usize, auto_consensus: bool, code_change_requested: bool, user_message: &str) -> ChatMessage {
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
             Do not play devil's advocate for its own sake. Only challenge when there is concrete evidence, missing evidence, a real risk, a wrong assumption, or a conflict with the user's requirement. \
             If there is no material disagreement, say so naturally, add only useful refinement, and move toward consensus. \
             No template labels like '共識狀態', '對 Hermes 的回應', or '對 OpenClaw 的回應' in visible text. \
             Use natural paragraphs, short headings when useful, and varied formatting; do not use rigid form-like layouts. \
             Consensus is only valid when both agents independently support the same concrete answer to the user's topic and list no blocking objections. \
             If consensus is not reached, focus on the exact unresolved decision instead of broad arguing. \
             If the same disagreement repeats without new evidence or a new risk, name the loop explicitly and either narrow it to a user decision or converge. \
             Hermes must be especially concise, but not bland. \
             When genuine consensus is reached, append the invisible marker '<!-- consensus:reached -->' at the very end. Do not mention this marker or show any consensus-status wording in visible text."
        ),
    }
}

fn debate_turn_instruction(agent: DebateAgent, turn_index: usize, is_first: bool, user_message: &str) -> ChatMessage {
    let other = agent.other().name();
    let instruction = if is_first {
        format!(
            "{}: You are the debate lead. Current user question/topic: '{}'. Reply in Traditional Chinese and answer that topic only. Do not acknowledge or summarize these debate instructions. Think independently and argue naturally, but do not force disagreement. Ground claims in project files/history, errors, commands, or explicit user requirements. If the situation is straightforward, move toward a concrete shared plan instead of inventing a fight. Use paragraphs and readable markdown, not a fixed template. Do not write visible labels like 共識狀態 or 對某某的回應. If genuine consensus is reached, append '<!-- consensus:reached -->' at the very end only.",
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

    ChatMessage { role: "user".into(), content: instruction }
}

fn final_instruction(agent: DebateAgent, code_change_requested: bool, reached_consensus: bool, user_message: &str) -> ChatMessage {
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
            "{}: Reply in Traditional Chinese. Produce the Final synthesis for the current user question only: '{}'. {code_rule} {consensus_rule} Do not summarize unrelated prior conversation topics, agent behavior rules, project memories, or yesterday's decisions unless the current question explicitly asks for them. Keep it readable and human, not a rigid form. Use concise sections only where they help: conclusion, evidence, decision, next steps, verification. Do not hide disagreement or call compromise consensus. Do not include invisible consensus markers in the Final.",
            agent.name(),
            user_message
        ),
    }
}

fn consensus_reached(turns: &[(DebateAgent, String)]) -> bool {
    if turns.len() < 2 {
        return false;
    }

    let recent = turns.iter().rev().take(2)
        .map(|(_, text)| normalize_consensus_text(text))
        .collect::<Vec<_>>();

    let unresolved_markers = [
        "disagree", "不同意", "反對", "尚未", "未解", " unresolved",
        "however", "但是", "不過", "仍然", "concern", "疑慮", "風險仍", "不能接受",
        "需要使用者決策", "交給 user", "交給使用者", "缺少證據", "資訊不足",
    ];

    if recent.iter().any(|text| unresolved_markers.iter().any(|marker| text.contains(marker))) {
        return false;
    }

    recent.iter().all(|text| has_consensus_signal(text))
}

fn normalize_consensus_text(text: &str) -> String {
    text.to_lowercase()
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
        "沒有 material disagreement",
        "基本正確",
        "方向沒問題",
        "我支持",
        "同一個具體方向",
        "同一個具體方案",
        "可執行的共同",
        "收斂後",
        "結論很簡單",
    ].iter().any(|marker| text.contains(marker))
}

fn code_change_requested(user_message: &str) -> bool {
    let text = user_message.to_lowercase();
    [
        "改程式", "調整程式", "修改程式", "修 bug", "debug", "實作", "開發", "寫 code",
        "code", "diff", "patch", "修正", "新增", "重構", "build", "lint", "cargo", "npm",
    ].iter().any(|kw| text.contains(kw))
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

fn chat_with_turns(base: &[ChatMessage], turns: &[(DebateAgent, String)], instruction: ChatMessage) -> Vec<ChatMessage> {
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
    chat.push(ChatMessage { role: "user".into(), content: user_message.into() });

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
        AgentMode::Debate => {
            let lead = choose_debate_lead(history, user_message);
            let round_limit = debate_round_limit(config);
            let code_change = code_change_requested(user_message);
            let mut base = vec![debate_instruction(lead, round_limit, config.debate_auto_consensus, code_change, user_message)];
            base.extend(chat.clone());

            let mut turns: Vec<(DebateAgent, String)> = vec![];
            let mut current = lead;
            let mut turn_index = 0usize;
            let mut reached_consensus = false;
            loop {
                if turn_index >= round_limit {
                    break;
                }

                let ctx = chat_with_turns(
                    &base,
                    &turns,
                    debate_turn_instruction(current, turn_index, turn_index == 0, user_message),
                );
                let reply = run_agent(&openclaw, &hermes, current, ctx).await?;
                results.push((current.role().into(), reply.clone(), Some(current.name().into())));
                turns.push((current, reply));

                if config.debate_auto_consensus && consensus_reached(&turns) {
                    reached_consensus = true;
                    break;
                }

                current = current.other();
                turn_index += 1;
            }

            let final_agent = lead;
            let final_ctx = chat_with_turns(&base, &turns, final_instruction(final_agent, code_change, reached_consensus, user_message));
            let final_reply = run_agent(&openclaw, &hermes, final_agent, final_ctx).await?;
            results.push((final_agent.role().into(), final_reply, Some(final_agent.name().into())));
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
    let debate_lead = choose_debate_lead(history, user_message);
    let code_change = code_change_requested(user_message);
    let current_topic = user_message.to_string();
    let mut chat = messages_to_chat(&project, history, project_summary.as_deref());
    chat.push(ChatMessage { role: "user".into(), content: current_topic.clone() });

    Box::pin(async_stream::stream! {
        let openclaw = OpenClawClient::new(&config);
        let hermes = HermesClient::new(&config);

        match mode {
            AgentMode::OpenClawOnly => {
                let mut stream = openclaw.chat_stream(chat);
                while let Some(chunk) = stream.next().await {
                    yield ServerEvent::Chunk { agent: "OpenClaw".into(), content: chunk, round: None, phase: None };
                }
                yield ServerEvent::Done { agent: "OpenClaw".into(), round: None, phase: None };
            }
            AgentMode::HermesOnly => {
                let mut stream = hermes.chat_stream(chat);
                while let Some(chunk) = stream.next().await {
                    yield ServerEvent::Chunk { agent: "Hermes".into(), content: chunk, round: None, phase: None };
                }
                yield ServerEvent::Done { agent: "Hermes".into(), round: None, phase: None };
            }
            AgentMode::Debate => {
                let lead = debate_lead;
                let round_limit = debate_round_limit(&config);
                let mut base = vec![debate_instruction(lead, round_limit, config.debate_auto_consensus, code_change, &current_topic)];
                base.extend(chat.clone());

                let mut turns: Vec<(DebateAgent, String)> = vec![];
                let mut current = lead;
                let mut turn_index = 0usize;
                let mut reached_consensus = false;

                loop {
                    if turn_index >= round_limit {
                        break;
                    }

                    let ctx = chat_with_turns(
                        &base,
                        &turns,
                        debate_turn_instruction(current, turn_index, turn_index == 0, &current_topic),
                    );

                    let mut buffer = String::new();
                    let mut stream = match current {
                        DebateAgent::OpenClaw => openclaw.chat_stream(ctx),
                        DebateAgent::Hermes => hermes.chat_stream(ctx),
                    };
                    while let Some(chunk) = stream.next().await {
                        buffer.push_str(&chunk);
                        yield ServerEvent::Chunk { agent: current.name().into(), content: chunk, round: Some(turn_index + 1), phase: Some("round".into()) };
                    }
                    yield ServerEvent::Done { agent: current.name().into(), round: Some(turn_index + 1), phase: Some("round".into()) };

                    turns.push((current, buffer));

                    if config.debate_auto_consensus && consensus_reached(&turns) {
                        reached_consensus = true;
                        break;
                    }

                    current = current.other();
                    turn_index += 1;
                }

                let final_agent = lead;
                let final_ctx = chat_with_turns(&base, &turns, final_instruction(final_agent, code_change, reached_consensus, &current_topic));
                let mut stream = match final_agent {
                    DebateAgent::OpenClaw => openclaw.chat_stream(final_ctx),
                    DebateAgent::Hermes => hermes.chat_stream(final_ctx),
                };
                while let Some(chunk) = stream.next().await {
                    yield ServerEvent::Chunk { agent: final_agent.name().into(), content: chunk, round: None, phase: Some("final".into()) };
                }
                yield ServerEvent::Done { agent: final_agent.name().into(), round: None, phase: Some("final".into()) };
            }
        }
    })
}
