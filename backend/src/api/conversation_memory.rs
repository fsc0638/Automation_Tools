use anyhow::Result as AnyResult;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    agents::{
        openclaw::{ChatMessage, OpenClawClient},
        orchestrator::ProjectScope,
    },
    config::Config,
    db::models::{Message, ProjectMemorySummary},
};

const PROJECT_HISTORY_LIMIT: i64 = 120;
const SUMMARY_SOURCE_LIMIT: i64 = 40;
const SUMMARY_CHAR_BUDGET: usize = 14_000;
const SUMMARY_MAX_CHARS: usize = 2_400;

pub async fn load_project_history(
    db: &PgPool,
    project_id: Uuid,
) -> Result<Vec<Message>, sqlx::Error> {
    let mut rows: Vec<Message> = sqlx::query_as(
        "SELECT m.*
         FROM messages m
         INNER JOIN conversations c ON c.id = m.conversation_id
         WHERE c.project_id = $1
         ORDER BY m.created_at DESC
         LIMIT $2",
    )
    .bind(project_id)
    .bind(PROJECT_HISTORY_LIMIT)
    .fetch_all(db)
    .await?;

    rows.reverse();
    Ok(rows)
}

pub async fn get_project_summary(
    db: &PgPool,
    project_id: Uuid,
) -> Result<Option<ProjectMemorySummary>, sqlx::Error> {
    sqlx::query_as(
        "SELECT project_id, summary, source_message_count, updated_at
         FROM project_memory_summaries
         WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(db)
    .await
}

pub async fn refresh_project_summary(
    db: &PgPool,
    config: &Arc<Config>,
    project: &ProjectScope,
) -> AnyResult<Option<ProjectMemorySummary>> {
    let recent_messages = load_recent_project_messages_for_summary(db, project.id).await?;
    if recent_messages.is_empty() {
        return Ok(None);
    }

    let existing = get_project_summary(db, project.id).await?;
    let prompt = build_summary_prompt(project, existing.as_ref(), &recent_messages);
    let summary = OpenClawClient::new(config).chat(prompt).await?;
    let normalized = normalize_summary(&summary);
    if normalized.is_empty() {
        return Ok(existing);
    }

    let source_message_count = recent_messages.len() as i32;
    let record: ProjectMemorySummary = sqlx::query_as(
        "INSERT INTO project_memory_summaries (project_id, summary, source_message_count, updated_at)
         VALUES ($1, $2, $3, NOW())
         ON CONFLICT (project_id)
         DO UPDATE SET summary = EXCLUDED.summary,
                       source_message_count = EXCLUDED.source_message_count,
                       updated_at = NOW()
         RETURNING project_id, summary, source_message_count, updated_at",
    )
    .bind(project.id)
    .bind(&normalized)
    .bind(source_message_count)
    .fetch_one(db)
    .await?;

    Ok(Some(record))
}

async fn load_recent_project_messages_for_summary(
    db: &PgPool,
    project_id: Uuid,
) -> Result<Vec<Message>, sqlx::Error> {
    let mut rows: Vec<Message> = sqlx::query_as(
        "SELECT m.*
         FROM messages m
         INNER JOIN conversations c ON c.id = m.conversation_id
         WHERE c.project_id = $1
         ORDER BY m.created_at DESC
         LIMIT $2",
    )
    .bind(project_id)
    .bind(SUMMARY_SOURCE_LIMIT)
    .fetch_all(db)
    .await?;

    rows.reverse();
    Ok(rows)
}

fn build_summary_prompt(
    project: &ProjectScope,
    existing: Option<&ProjectMemorySummary>,
    recent_messages: &[Message],
) -> Vec<ChatMessage> {
    let existing_summary = existing
        .map(|s| s.summary.as_str())
        .unwrap_or("[No previous summary]");
    let root = project.root.as_deref().unwrap_or("unknown");
    let recent_transcript = render_recent_messages(recent_messages);

    vec![
        ChatMessage {
            role: "system".into(),
            content: "You are OpenClaw maintaining durable project memory for a multi-agent coding workspace. Respond in Traditional Chinese. Produce a concise but information-dense markdown summary that future Hermes/OpenClaw turns can load as project memory. Do not distort, invent, or promote unresolved debate into consensus. Focus only on durable facts grounded in the transcript: architecture, confirmed decisions, accepted constraints, important file paths, unresolved questions, user preferences, and recent meaningful changes. Exclude chatter, duplicated reasoning, and ephemeral phrasing. If facts conflict, call out the conflict explicitly instead of guessing. Keep the summary under 12 bullets and under 2400 characters.".into(),
        },
        ChatMessage {
            role: "user".into(),
            content: format!(
                "Project id: {}\nProject name: {}\nProject root: {}\n\nExisting project memory summary:\n{}\n\nRecent cross-conversation transcript excerpt:\n{}\n\nRewrite the project memory summary for future turns. Use this structure:\n## Current state\n- ...\n## Stable decisions / constraints\n- ...\n## Open questions / risks\n- ...\n## Recent notable changes\n- ...\nOnly include bullets that are grounded in the provided transcript or project metadata.",
                project.id,
                project.name,
                root,
                existing_summary,
                recent_transcript,
            ),
        },
    ]
}

fn render_recent_messages(messages: &[Message]) -> String {
    let mut output = String::new();
    for message in messages {
        let label = match message.role.as_str() {
            "user" => "User",
            "hermes" => message.agent_name.as_deref().unwrap_or("Hermes"),
            "openclaw" => message.agent_name.as_deref().unwrap_or("OpenClaw"),
            "system" => message.agent_name.as_deref().unwrap_or("System"),
            _ => "Unknown",
        };
        let sanitized = message.content.replace("\r", "").replace("\n", " ");
        output.push_str("- ");
        output.push_str(label);
        output.push_str(": ");
        output.push_str(&sanitized);
        output.push('\n');
        if output.len() >= SUMMARY_CHAR_BUDGET {
            output.truncate(SUMMARY_CHAR_BUDGET);
            output.push_str("\n...[truncated]");
            break;
        }
    }
    output
}

fn normalize_summary(summary: &str) -> String {
    let normalized = summary.replace("\r\n", "\n").trim().to_string();
    if normalized.chars().count() <= SUMMARY_MAX_CHARS {
        normalized
    } else {
        normalized
            .chars()
            .take(SUMMARY_MAX_CHARS)
            .collect::<String>()
            .trim()
            .to_string()
    }
}
