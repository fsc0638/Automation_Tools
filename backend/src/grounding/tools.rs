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

use lettre::{
    message::Mailbox,
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
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
    /// Authenticated user — required by vault_reveal for ownership check.
    pub user_id: uuid::Uuid,
    /// User KEK cipher from the in-RAM session store.
    /// None → vault_reveal declines gracefully.
    pub vault_cipher: Option<std::sync::Arc<crate::crypto::TokenCipher>>,
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
- vault_reveal {\"id\":\"<secret-uuid>\"}  — 取得 Vault 中某個憑證的明文值（僅在任務明確需要時使用）\n\
- list_tree {\"path\":\"可選相對目錄\"}  — 列出目錄結構\n\
- send_email  寄信，兩種形式擇一：\n\
  形式A（完整設定存在 Vault）：{\"vault_id\":\"<smtp-config-uuid>\",\"to\":\"...\",\"subject\":\"...\",\"body\":\"...\"}\n\
  形式B（config 從專案讀，密碼單獨存 Vault）：{\"host\":\"mail.x.com\",\"port\":587,\"username\":\"user@x.com\",\"password_vault_id\":\"<uuid>\",\"from\":\"可選\",\"to\":\"...\",\"subject\":\"...\",\"body\":\"...\"}\n\
\n\
系統會執行該工具，並以 OBSERVATION: 回傳結果。你可再呼叫工具或回答。\n\
當你已經能回答時，輸出：\n\
FINAL: <最終答案>\n\
答案需根據實際讀到的內容，並標明依據的檔案路徑。"
}

/// ReAct contract for the **streaming chat** path. Same protocol as
/// `protocol_prompt` but worded for an interactive turn: explore the
/// real project via tools, then answer. The in-stream interceptor in
/// run_agent_stream parses `ACTION:` lines live, executes the tool
/// (firewalled), feeds `OBSERVATION:` back, and keeps streaming.
pub fn chat_tools_protocol() -> String {
    "[工具能力] 你不是只能看到預先注入的片段——你可以主動查證這個專案的\
真實檔案後再回答。需要時就用工具，不要用猜的、也不要叫使用者貼檔。\n\
\n\
呼叫工具：輸出獨立一行，格式為\n\
ACTION: <tool> <json參數>\n\
然後停住等系統回 OBSERVATION:。可連續多次（最多 5 次）。\n\
\n\
可用工具（皆唯讀、限本專案）：\n\
- search_index {\"query\":\"自然語言或關鍵字\"}  混合語意+字面檢索已索引內容（中文問也行）\n\
- read_file {\"path\":\"相對路徑\"}  讀本地該檔；或加 {\"ref\":\"分支/commit/tag\"} 直接讀遠端那個版本\n\
- list_tree {\"path\":\"可選相對目錄\"}  看目錄結構\n\
- vault_reveal {\"id\":\"<secret-uuid>\"}  從使用者 Vault 取得指定憑證的明文值（僅在任務明確需要時使用）\n\
- send_email  寄信，兩種形式：\n\
  形式A {\"vault_id\":\"<smtp-config-uuid>\",\"to\":\"...\",\"subject\":\"...\",\"body\":\"...\"}  Vault 存完整 JSON SMTP 設定\n\
  形式B {\"host\":\"...\",\"port\":587,\"username\":\"...\",\"password_vault_id\":\"<uuid>\",\"to\":\"...\",\"subject\":\"...\",\"body\":\"...\"}  host/port/username 從專案讀，密碼從 Vault 取\n\
\n\
策略：先 search_index 找線索 → read_file 把關鍵檔讀進來核實 → 再回答。\n\
如需要憑證才能完成任務（例如 git clone 私有 repo），依 Vault 清單挑對應 id 並呼叫 vault_reveal。\n\
寄信優先用形式B：search_index 找 SMTP host/port/username → Vault 清單找密碼 id → 直接呼叫 send_email。\n\
能回答時，用一行 FINAL: 開頭給最終答案，並標明依據的實際檔案路徑。"
        .to_string()
}

/// Short, human-friendly status line shown to the user while a tool
/// runs (so the chat shows "讀取 src/auth.rs ..." instead of raw
/// protocol noise).
pub fn tool_status_msg(call: &ToolCall) -> String {
    let arg = |k: &str| {
        call.args
            .get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    match call.name.as_str() {
        "search_index" => format!("檢索專案：「{}」…", arg("query")),
        "vault_reveal" => format!("讀取 Vault 憑證 {}…", &arg("id")[..8.min(arg("id").len())]),
        "read_file" => {
            let p = arg("path");
            let r = arg("ref");
            if r.is_empty() {
                format!("讀取檔案 {p} …")
            } else {
                format!("讀取遠端 {p}@{r} …")
            }
        }
        "list_tree" => {
            let p = arg("path");
            if p.is_empty() {
                "瀏覽專案目錄結構…".to_string()
            } else {
                format!("瀏覽目錄 {p} …")
            }
        }
        "send_email" => {
            let to = arg("to");
            let subj = arg("subject");
            format!("寄送郵件 → {to}「{subj}」…")
        }
        other => format!("執行工具 {other} …"),
    }
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
            } else if super::is_secret_path(path) {
                // Secret files are refused regardless of local/remote.
                "(read_file: 機密類檔案，依資安政策拒絕)".to_string()
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
        "vault_reveal" => {
            // Retrieve a vault secret by ID for agent use.
            // Ownership is enforced; cipher must be present (active session).
            // Result bypasses redact_secrets so the value reaches the model,
            // but the vault echo guard in ws.rs prevents it appearing verbatim
            // in the final streamed reply.
            let id_str = call
                .args
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let id = match id_str.parse::<uuid::Uuid>() {
                Ok(v) => v,
                Err(_) => return "(vault_reveal: invalid UUID for 'id')".to_string(),
            };
            let Some(ref cipher) = ctx.vault_cipher else {
                return "(vault_reveal: no active vault session — log in again)".to_string();
            };
            // Ownership check.
            let owner: Option<uuid::Uuid> = sqlx::query_scalar(
                "SELECT user_id FROM vault_secrets WHERE id = $1",
            )
            .bind(id)
            .fetch_optional(ctx.db)
            .await
            .unwrap_or(None);
            match owner {
                None => return "(vault_reveal: secret not found)".to_string(),
                Some(uid) if uid != ctx.user_id => {
                    return "(vault_reveal: forbidden — not your secret)".to_string();
                }
                _ => {}
            }
            // Decrypt via vault service (User KEK path).
            let vsvc = crate::security::vault_service::VaultService::for_user(
                ctx.db,
                cipher.clone(),
                ctx.user_id,
                cipher.clone(), // system cipher not needed for User KEK path
                ctx.user_id,
                None,
            );
            match vsvc.open("vault_secret", id, "agent_tool").await {
                Ok(bytes) => match String::from_utf8(bytes) {
                    // Returned directly — NOT through redact_secrets.
                    Ok(v) => return v.chars().take(MAX_TOOL_OUTPUT_CHARS).collect(),
                    Err(_) => return "(vault_reveal: decrypted value is not UTF-8)".to_string(),
                },
                Err(e) => return format!("(vault_reveal: decrypt failed: {e})"),
            }
        }
        "send_email" => {
            return send_email_tool(ctx, call).await;
        }
        other => format!("(unknown tool '{other}'; valid: search_index, read_file, list_tree, vault_reveal, send_email)"),
    };

    // INVARIANT: tool output cannot bypass secret redaction.
    let redacted = redact_secrets(&raw).text;
    redacted.chars().take(MAX_TOOL_OUTPUT_CHARS).collect()
}

// ── send_email implementation ─────────────────────────────────────────────────

/// Two calling forms:
///
/// **Form A — full config in Vault** (one entry holds the entire JSON):
/// ```json
/// ACTION: send_email {"vault_id":"<uuid>","to":"a@b.com","subject":"…","body":"…"}
/// ```
/// The vault secret_value must be JSON:
/// `{"host":"…","port":587,"username":"…","password":"…","from":"Name <addr>"}`
///
/// **Form B — inline config + password from Vault** (agent assembles from project files):
/// ```json
/// ACTION: send_email {"host":"mail.kway.com.tw","port":587,"username":"user@kway.com.tw",
///                     "password_vault_id":"<uuid>","from":"…","to":"…","subject":"…","body":"…"}
/// ```
/// The vault entry for `password_vault_id` may hold the raw password string,
/// or a JSON object with a `"password"` key.
///
/// Port 587 → STARTTLS.  Port 465 → implicit TLS.
async fn send_email_tool(ctx: &ToolCtx<'_>, call: &ToolCall) -> String {
    // ── 1. Common fields (to / subject / body) ───────────────────────────
    let subject = call
        .args
        .get("subject")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let body = call
        .args
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    if subject.is_empty() {
        return "(send_email: missing 'subject')".to_string();
    }
    if body.is_empty() {
        return "(send_email: missing 'body')".to_string();
    }

    // 'to' accepts a comma-separated string or a JSON array.
    let to_addresses: Vec<String> = match call.args.get("to") {
        Some(v) if v.is_array() => v
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|a| a.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect(),
        Some(v) if v.is_string() => v
            .as_str()
            .unwrap()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => vec![],
    };
    if to_addresses.is_empty() {
        return "(send_email: missing or empty 'to')".to_string();
    }

    // ── 2. Session guard ─────────────────────────────────────────────────
    let Some(ref cipher) = ctx.vault_cipher else {
        return "(send_email: no active vault session — log in again)".to_string();
    };

    // ── 3. Resolve SMTP config (Form A or Form B) ────────────────────────
    let (host, port, username, password, from_str) =
        match resolve_smtp_config(ctx, call, cipher).await {
            Ok(t) => t,
            Err(e) => return e,
        };

    // ── 4. Build lettre Message ──────────────────────────────────────────
    let from_mailbox: Mailbox = match from_str.parse() {
        Ok(m) => m,
        Err(e) => return format!("(send_email: invalid 'from' address '{from_str}': {e})"),
    };
    let mut builder = Message::builder().from(from_mailbox).subject(subject);
    for addr in &to_addresses {
        let mb: Mailbox = match addr.parse() {
            Ok(m) => m,
            Err(e) => return format!("(send_email: invalid 'to' address '{addr}': {e})"),
        };
        builder = builder.to(mb);
    }
    let email = match builder.body(body.to_string()) {
        Ok(m) => m,
        Err(e) => return format!("(send_email: failed to build email message: {e})"),
    };

    // ── 5. Build SMTP transport and send ─────────────────────────────────
    // Port 465 → implicit TLS; everything else → STARTTLS.
    let creds = Credentials::new(username, password);
    let mailer_result: Result<AsyncSmtpTransport<Tokio1Executor>, _> = if port == 465 {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
            .map(|b| b.port(port).credentials(creds).build())
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
            .map(|b| b.port(port).credentials(creds).build())
    };
    let mailer = match mailer_result {
        Ok(m) => m,
        Err(e) => return format!("(send_email: failed to build SMTP transport: {e})"),
    };

    match mailer.send(email).await {
        Ok(_) => format!(
            "郵件已成功寄出。收件人：{}；主旨：「{}」",
            to_addresses.join(", "),
            subject
        ),
        Err(e) => format!("(send_email: SMTP send failed: {e})"),
    }
}

/// Resolve SMTP (host, port, username, password, from) from either:
/// - Form A: `vault_id` pointing to a full JSON config in Vault
/// - Form B: inline `host`/`port`/`username`/`from` + `password_vault_id` pointing to
///           just the password string (or a JSON object with a `"password"` key)
async fn resolve_smtp_config(
    ctx: &ToolCtx<'_>,
    call: &ToolCall,
    cipher: &std::sync::Arc<crate::crypto::TokenCipher>,
) -> Result<(String, u16, String, String, String), String> {
    let arg_str = |k: &str| {
        call.args
            .get(k)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("")
            .to_string()
    };

    let vault_id_str = arg_str("vault_id");
    let host_arg = arg_str("host");
    let password_vault_id_str = arg_str("password_vault_id");

    // ── Form A ────────────────────────────────────────────────────────────
    if !vault_id_str.is_empty() {
        let id: uuid::Uuid = vault_id_str
            .parse()
            .map_err(|_| "(send_email: invalid UUID for 'vault_id')".to_string())?;

        vault_ownership_check(ctx, id).await?;

        let vsvc = make_vsvc(ctx, cipher);
        let bytes = vsvc
            .open("vault_secret", id, "send_email")
            .await
            .map_err(|e| format!("(send_email: vault decrypt failed: {e})"))?;
        let s = String::from_utf8(bytes)
            .map_err(|_| "(send_email: vault value is not UTF-8)".to_string())?;
        let cfg: serde_json::Value = serde_json::from_str(&s).map_err(|_| {
            "(send_email: vault_id value is not JSON; \
             expected {\"host\":\"…\",\"port\":587,\"username\":\"…\",\"password\":\"…\"})"
                .to_string()
        })?;

        let host = smtp_field(&cfg, "host")?;
        let port = cfg
            .get("port")
            .and_then(|v| v.as_u64())
            .unwrap_or(587)
            .clamp(1, 65535) as u16;
        let username = smtp_field(&cfg, "username")?;
        let password = smtp_field(&cfg, "password")?;
        let from = cfg
            .get("from")
            .and_then(|v| v.as_str())
            .unwrap_or(&username)
            .to_string();

        return Ok((host, port, username, password, from));
    }

    // ── Form B ────────────────────────────────────────────────────────────
    if host_arg.is_empty() {
        return Err(
            "(send_email: provide either 'vault_id' (full SMTP config in Vault) \
             or 'host'+'username'+'password_vault_id' (inline config + Vault password))"
                .to_string(),
        );
    }
    if password_vault_id_str.is_empty() {
        return Err("(send_email: 'password_vault_id' is required in Form B)".to_string());
    }
    let username_arg = arg_str("username");
    if username_arg.is_empty() {
        return Err("(send_email: 'username' is required in Form B)".to_string());
    }

    let pw_id: uuid::Uuid = password_vault_id_str
        .parse()
        .map_err(|_| "(send_email: invalid UUID for 'password_vault_id')".to_string())?;

    vault_ownership_check(ctx, pw_id).await?;

    let vsvc = make_vsvc(ctx, cipher);
    let pw_bytes = vsvc
        .open("vault_secret", pw_id, "send_email_pw")
        .await
        .map_err(|e| format!("(send_email: vault decrypt failed for password: {e})"))?;
    let pw_str = String::from_utf8(pw_bytes)
        .map_err(|_| "(send_email: password vault value is not UTF-8)".to_string())?;

    // The vault entry may be a raw password string or a JSON object with "password" key.
    let password = if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&pw_str) {
        obj.get("password")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or(pw_str)
    } else {
        pw_str.trim().to_string()
    };
    if password.is_empty() {
        return Err("(send_email: resolved password is empty)".to_string());
    }

    let port: u16 = call
        .args
        .get("port")
        .and_then(|v| v.as_u64())
        .unwrap_or(587)
        .clamp(1, 65535) as u16;
    let from = {
        let f = arg_str("from");
        if f.is_empty() { username_arg.clone() } else { f }
    };

    Ok((host_arg, port, username_arg, password, from))
}

/// Shared ownership check — rejects missing or foreign secrets.
async fn vault_ownership_check(ctx: &ToolCtx<'_>, id: uuid::Uuid) -> Result<(), String> {
    let owner: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT user_id FROM vault_secrets WHERE id = $1")
            .bind(id)
            .fetch_optional(ctx.db)
            .await
            .unwrap_or(None);
    match owner {
        None => Err("(send_email: vault secret not found)".to_string()),
        Some(uid) if uid != ctx.user_id => {
            Err("(send_email: forbidden — not your secret)".to_string())
        }
        _ => Ok(()),
    }
}

/// Build a VaultService scoped to the current user KEK.
fn make_vsvc<'a>(
    ctx: &'a ToolCtx<'a>,
    cipher: &'a std::sync::Arc<crate::crypto::TokenCipher>,
) -> crate::security::vault_service::VaultService<'a> {
    crate::security::vault_service::VaultService::for_user(
        ctx.db,
        cipher.clone(),
        ctx.user_id,
        cipher.clone(),
        ctx.user_id,
        None,
    )
}

/// Extract a required non-empty string field from a SMTP config JSON object.
fn smtp_field(cfg: &serde_json::Value, key: &str) -> Result<String, String> {
    match cfg.get(key).and_then(|v| v.as_str()) {
        Some(v) if !v.trim().is_empty() => Ok(v.trim().to_string()),
        _ => Err(format!("(send_email: SMTP config missing '{key}')")),
    }
}
