use anyhow::Result;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    agents::orchestrator::ProjectScope,
    db::models::Message,
    security::redaction::{classify_path, redact_secrets, DataClassification},
};

#[derive(Debug, Clone)]
pub struct SecuredAgentContext {
    pub project_scope: ProjectScope,
    pub history: Vec<Message>,
    pub project_summary: Option<String>,
    pub user_message: String,
    pub report: ContextFirewallReport,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ContextFirewallReport {
    pub redacted_count: usize,
    pub blocked_items: Vec<String>,
    pub included_files: Vec<String>,
    pub classification_max: String,
    pub outbound_context_hash: String,
    pub token_estimate: i32,
}

#[derive(Debug, Default)]
struct FirewallAccumulator {
    redacted_count: usize,
    blocked_items: Vec<String>,
    included_files: Vec<String>,
    classification_max: DataClassification,
    outbound_parts: Vec<String>,
}

impl Default for DataClassification {
    fn default() -> Self {
        DataClassification::Public
    }
}

pub async fn secure_agent_context(
    db: &PgPool,
    user_id: Uuid,
    project_id: Uuid,
    conversation_id: Uuid,
    agent_mode: &str,
    project_scope: &ProjectScope,
    history: &[Message],
    project_summary: Option<String>,
    user_message: &str,
) -> Result<SecuredAgentContext> {
    let mut acc = FirewallAccumulator::default();

    let mut secured_scope = project_scope.clone();
    secured_scope.file_snapshot = project_scope.file_snapshot.as_deref().map(|text| {
        sanitize_file_block_context("file_snapshot", text, FileBlockKind::Snapshot, &mut acc)
    });
    secured_scope.relevant_file_context =
        project_scope.relevant_file_context.as_deref().map(|text| {
            sanitize_file_block_context(
                "relevant_file_context",
                text,
                FileBlockKind::Indexed,
                &mut acc,
            )
        });

    let secured_summary =
        project_summary.map(|summary| sanitize_plain("project_summary", &summary, &mut acc));
    let secured_user_message = sanitize_plain("user_message", user_message, &mut acc);
    let secured_history = history
        .iter()
        .map(|message| {
            let mut m = message.clone();
            m.content = sanitize_plain("history_message", &message.content, &mut acc);
            m
        })
        .collect::<Vec<_>>();

    let combined = acc.outbound_parts.join("\n---SECTION---\n");
    let hash = format!("{:x}", Sha256::digest(combined.as_bytes()));
    let token_estimate = estimate_tokens(&combined);
    let report = ContextFirewallReport {
        redacted_count: acc.redacted_count,
        blocked_items: acc.blocked_items.clone(),
        included_files: acc.included_files.clone(),
        classification_max: acc.classification_max.as_str().to_string(),
        outbound_context_hash: hash,
        token_estimate,
    };

    write_context_audit(
        db,
        user_id,
        project_id,
        conversation_id,
        agent_mode,
        &report,
    )
    .await?;

    Ok(SecuredAgentContext {
        project_scope: secured_scope,
        history: secured_history,
        project_summary: secured_summary,
        user_message: secured_user_message,
        report,
    })
}

async fn write_context_audit(
    db: &PgPool,
    user_id: Uuid,
    project_id: Uuid,
    conversation_id: Uuid,
    agent_mode: &str,
    report: &ContextFirewallReport,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO agent_context_audit_logs
         (user_id, project_id, conversation_id, agent_mode, outbound_context_hash,
          included_files, blocked_items, redacted_count, classification_max, token_estimate)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(user_id)
    .bind(project_id)
    .bind(conversation_id)
    .bind(agent_mode)
    .bind(&report.outbound_context_hash)
    .bind(json!(&report.included_files))
    .bind(json!(&report.blocked_items))
    .bind(report.redacted_count as i32)
    .bind(&report.classification_max)
    .bind(report.token_estimate)
    .execute(db)
    .await?;
    Ok(())
}

fn sanitize_plain(label: &str, text: &str, acc: &mut FirewallAccumulator) -> String {
    let sanitized = redact_secrets(text);
    acc.redacted_count += sanitized.report.redacted_count;
    acc.classification_max = acc.classification_max.max(sanitized.classification);
    acc.outbound_parts
        .push(format!("[{label}]\n{}", sanitized.text));
    sanitized.text
}

#[derive(Debug, Clone, Copy)]
enum FileBlockKind {
    Snapshot,
    Indexed,
}

fn sanitize_file_block_context(
    label: &str,
    text: &str,
    kind: FileBlockKind,
    acc: &mut FirewallAccumulator,
) -> String {
    let mut output = String::new();
    let mut current_header: Option<String> = None;
    let mut current_path: Option<String> = None;
    let mut current_body = String::new();

    for line in text.lines() {
        if let Some(path) = extract_block_path(line, kind) {
            flush_file_block(
                &mut output,
                &mut current_header,
                &mut current_path,
                &mut current_body,
                acc,
            );
            current_header = Some(line.to_string());
            current_path = Some(path);
            continue;
        }

        if current_header.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }

    flush_file_block(
        &mut output,
        &mut current_header,
        &mut current_path,
        &mut current_body,
        acc,
    );

    // Redact any secrets in non-file prose such as file-tree listings.
    let sanitized = redact_secrets(&output);
    acc.redacted_count += sanitized.report.redacted_count;
    acc.classification_max = acc.classification_max.max(sanitized.classification);
    acc.outbound_parts
        .push(format!("[{label}]\n{}", sanitized.text));
    sanitized.text
}

fn flush_file_block(
    output: &mut String,
    current_header: &mut Option<String>,
    current_path: &mut Option<String>,
    current_body: &mut String,
    acc: &mut FirewallAccumulator,
) {
    let Some(header) = current_header.take() else {
        return;
    };
    let path = current_path.take().unwrap_or_else(|| "unknown".into());
    let path_classification = classify_path(&path);
    acc.classification_max = acc.classification_max.max(path_classification);

    if path_classification >= DataClassification::Restricted {
        acc.blocked_items.push(path.clone());
        output.push_str(&header);
        output.push('\n');
        output.push_str("[BLOCKED_BY_CONTEXT_FIREWALL: restricted or secret file path]\n");
        current_body.clear();
        return;
    }

    let sanitized = redact_secrets(current_body);
    acc.redacted_count += sanitized.report.redacted_count;
    acc.classification_max = acc.classification_max.max(sanitized.classification);
    acc.included_files.push(path);
    output.push_str(&header);
    output.push('\n');
    output.push_str(&sanitized.text);
    if !sanitized.text.ends_with('\n') {
        output.push('\n');
    }
    current_body.clear();
}

fn extract_block_path(line: &str, kind: FileBlockKind) -> Option<String> {
    match kind {
        FileBlockKind::Snapshot => {
            let rest = line.strip_prefix("--- FILE: ")?;
            Some(rest.trim_end_matches(" ---").trim().to_string())
        }
        FileBlockKind::Indexed => {
            let rest = line.strip_prefix("--- INDEXED FILE: ")?;
            let path = rest.split(" [chunk ").next().unwrap_or(rest).trim();
            Some(path.to_string())
        }
    }
}

fn estimate_tokens(text: &str) -> i32 {
    let ascii = text.chars().filter(|c| c.is_ascii()).count() as f64;
    let non_ascii = text.chars().filter(|c| !c.is_ascii()).count() as f64;
    ((ascii / 4.0) + non_ascii).ceil() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_context_blocks_secret_paths_and_redacts_remaining_chunks() {
        let mut acc = FirewallAccumulator::default();
        let input = "Relevant files\n--- INDEXED FILE: .env [chunk 0] ---\nOPENAI_API_KEY=sk-secretsecretsecretsecret\n--- INDEXED FILE: src/main.rs [chunk 1] ---\nlet token = \"ghp_abcdefghijklmnopqrstuvwxyz\";\n";

        let output =
            sanitize_file_block_context("relevant", input, FileBlockKind::Indexed, &mut acc);
        assert!(output.contains("[BLOCKED_BY_CONTEXT_FIREWALL"));
        assert!(!output.contains("sk-secret"));
        assert!(!output.contains("ghp_abcdefghijklmnopqrstuvwxyz"));
        assert_eq!(acc.blocked_items, vec![".env".to_string()]);
        assert_eq!(acc.included_files, vec!["src/main.rs".to_string()]);
        assert!(acc.redacted_count >= 1);
    }

    #[test]
    fn snapshot_context_blocks_restricted_key_files() {
        let mut acc = FirewallAccumulator::default();
        let input = "Tree\n--- FILE: keys/service.pem ---\n-----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----\n--- FILE: README.md ---\nhello\n";
        let output =
            sanitize_file_block_context("snapshot", input, FileBlockKind::Snapshot, &mut acc);
        assert!(output.contains("keys/service.pem"));
        assert!(output.contains("[BLOCKED_BY_CONTEXT_FIREWALL"));
        assert!(!output.contains("BEGIN PRIVATE KEY"));
        assert_eq!(acc.included_files, vec!["README.md".to_string()]);
    }
}
