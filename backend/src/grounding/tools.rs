//! Phase 5 — ReAct text-protocol tool calling.
//!
//! ## Why a text protocol, not OpenAI tool_calls
//!
//! Phase 0 spike proved the gateways do **not** support client-side
//! `tool_calls`: Hermes/OpenClaw are autonomous agents that run their
//! own sandboxed tools and ignore a `tools` array we send. So the model
//! can only "call a tool" by emitting agreed-upon **text** that the
//! backend intercepts, executes, and feeds back — the classic ReAct
//! loop. This module is that protocol: the prompt contract, a tolerant
//! parser, and the (small, safe, read-only) tool set.
//!
//! ## Safety invariants
//!
//! - Every tool is **read-only** (no writes, no shell).
//! - Path arguments are confined to the project root (no `..`, no
//!   absolute escape) — verified by canonicalisation.
//! - **Every** tool observation is passed through the firewall's secret
//!   redaction before it can re-enter the model context. The plan is
//!   explicit: "工具輸出一樣過 context_firewall（不能繞過遮密）".
//! - Output is truncated so a huge file can't blow the context window.
//! - The caller bounds the loop with a max-iteration guard.

use serde::Serialize;

use crate::api::project_index::relevant_file_context;
use crate::db::models::Project;
use crate::git_ops::manager::{list_files, read_file_content, GitCredentials};
use crate::security::redaction::redact_secrets;
use sqlx::PgPool;

/// Hard cap on a single observation fed back to the model.
pub const MAX_TOOL_OUTPUT_CHARS: usize = 6_000;

/// A parsed tool invocation requested by the model.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub name: String,
    pub args: serde_json::Value,
}

/// One executed ReAct step (for the API response / audit trail).
#[derive(Debug, Serialize)]
pub struct ToolStep {
    pub action: String,
    pub args: serde_json::Value,
    pub observation_chars: usize,
}

/// Everything a tool needs to run, resolved once per ReAct session.
pub struct ToolCtx<'a> {
    pub db: &'a PgPool,
    pub project: &'a Project,
    /// Project root on disk (already access-checked by the caller).
    pub root: String,
    pub credentials: Option<GitCredentials>,
}

/// The ReAct contract handed to the model as the system prompt.
pub fn protocol_prompt() -> &'static str {
    "你可以使用工具來查證專案內容，再回答。協定如下，務必嚴格遵守：\n\
\n\
要呼叫工具時，只輸出「一行」，格式為：\n\
ACTION: <tool> <json-參數>\n\
輸出該行後立刻停止，不要附加其他文字。\n\
\n\
可用工具：\n\
- search_index {\"query\":\"關鍵字或自然語言\"}  — 混合檢索專案已索引內容\n\
- read_file {\"path\":\"相對路徑\",\"ref\":\"可選 git ref\"}  — 讀單一檔案（給 ref 則直接讀遠端該版本）\n\
- list_tree {\"path\":\"可選相對目錄\"}  — 列出目錄結構\n\
\n\
系統會執行該工具，並以 OBSERVATION: 回傳結果。你可再呼叫工具或回答。\n\
當你已經能回答時，輸出：\n\
FINAL: <最終答案>\n\
答案需根據實際讀到的內容，並標明依據的檔案路徑。"
}

/// Parse the FIRST `ACTION:` directive from a model turn. Tolerates a
/// bare line or a ```fenced``` block, and a missing/!JSON arg blob
/// (treated as `{}`), so a slightly-off model turn still progresses.
pub fn parse_action(text: &str) -> Option<ToolCall> {
    let line = text.lines().map(str::trim).find_map(|l| {
        let l = l.trim_start_matches("```action").trim_start_matches("```").trim();
        l.strip_prefix("ACTION:").map(str::trim)
    })?;

    let mut it = line.splitn(2, char::is_whitespace);
    let name = it.next()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let args = it
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    Some(ToolCall { name, args })
}

/// Strip a leading `FINAL:` marker from the model's terminal answer.
pub fn strip_final(text: &str) -> String {
    let t = text.trim();
    t.find("FINAL:")
        .map(|i| t[i + "FINAL:".len()..].trim().to_string())
        .unwrap_or_else(|| t.to_string())
}

/// Confine a model-supplied relative path to the project root. Returns
/// the absolute, canonicalised path or `None` if it escapes the root
/// (absolute path, `..`, symlink-out, etc.). This is the guard that
/// keeps `read_file` / `list_tree` from reading arbitrary disk.
fn confined_path(root: &str, rel: &str) -> Option<std::path::PathBuf> {
    let rel = rel.trim().trim_start_matches(['/', '\\']);
    if rel.split(['/', '\\']).any(|seg| seg == "..") {
        return None;
    }
    let root_canon = std::fs::canonicalize(root).ok()?;
    let joined = if rel.is_empty() {
        root_canon.clone()
    } else {
        root_canon.join(rel)
    };
    let canon = std::fs::canonicalize(&joined).ok()?;
    canon.starts_with(&root_canon).then_some(canon)
}

fn render_tree(nodes: &[crate::api::projects::FileNode], depth: usize, out: &mut String) {
    if depth > 3 {
        return;
    }
    for n in nodes {
        for _ in 0..depth {
            out.push_str("  ");
        }
        out.push_str(if n.is_dir { "📁 " } else { "📄 " });
        out.push_str(&n.path);
        out.push('\n');
        if let Some(children) = &n.children {
            render_tree(children, depth + 1, out);
        }
        if out.len() > MAX_TOOL_OUTPUT_CHARS {
            return;
        }
    }
}

/// Execute one tool and return its (redacted, truncated) observation.
/// Never errors — failures become an observation string the model can
/// reason about, so the loop always makes progress.
pub async fn run_tool(ctx: &ToolCtx<'_>, call: &ToolCall) -> String {
    let raw = match call.name.as_str() {
        "search_index" => {
            let q = call
                .args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if q.is_empty() {
                "(search_index: missing 'query')".to_string()
            } else {
                relevant_file_context(ctx.db, ctx.project.id, q)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "(no relevant indexed context found)".to_string())
            }
        }
        "read_file" => {
            let path = call
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let git_ref = call
                .args
                .get("ref")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if path.is_empty() {
                "(read_file: missing 'path')".to_string()
            } else if let Some(r) = git_ref {
                // ref given ⇒ Phase 2b live remote read (RemoteLive).
                match crate::grounding::remote_file(
                    ctx.project,
                    ctx.credentials.as_ref(),
                    path,
                    r,
                )
                .await
                {
                    Ok(c) => c,
                    Err(e) => format!("(remote read failed: {e})"),
                }
            } else if confined_path(&ctx.root, path).is_none() {
                "(read_file: path escapes project root — rejected)".to_string()
            } else {
                match read_file_content(&ctx.root, path) {
                    Ok(c) => c,
                    Err(e) => format!("(local read failed: {e})"),
                }
            }
        }
        "list_tree" => {
            let path = call
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            match confined_path(&ctx.root, path) {
                None => "(list_tree: path escapes project root — rejected)".to_string(),
                Some(dir) => match list_files(&ctx.root, &dir.to_string_lossy(), 0) {
                    Ok(nodes) => {
                        let mut out = String::new();
                        render_tree(&nodes, 0, &mut out);
                        if out.is_empty() {
                            "(empty)".to_string()
                        } else {
                            out
                        }
                    }
                    Err(e) => format!("(list_tree failed: {e})"),
                },
            }
        }
        other => format!("(unknown tool '{other}'; valid: search_index, read_file, list_tree)"),
    };

    // INVARIANT: tool output cannot bypass secret redaction.
    let redacted = redact_secrets(&raw).text;
    redacted.chars().take(MAX_TOOL_OUTPUT_CHARS).collect()
}
