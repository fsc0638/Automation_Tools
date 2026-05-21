use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
    /// Per-user Argon2id salt for User KEK derivation (migration 0042).
    /// Never sent to the client; zeroized implicitly when the struct drops.
    #[serde(skip_serializing)]
    #[sqlx(default)]
    pub kek_salt: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct Project {
    pub id: Uuid,
    pub user_id: Uuid,
    pub organization_id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub source_type: String,        // "local" | "git" | "upload"
    pub source_path: String,        // local path, git URL, or uploaded project path
    pub local_path: Option<String>, // cloned path for git repos
    pub default_branch: Option<String>,
    pub git_identity_id: Option<Uuid>,
    /// Workspace kind (migration 0038): "code" (repo-backed, status
    /// quo) | "admin" | "general" (行政庶務 / personal — no repo).
    /// Backend behaviour is binary on `kind == "code"`; admin/general
    /// are UI-only categories. NOT NULL DEFAULT 'code' so every legacy
    /// row and every `SELECT *` / `RETURNING *` path is unaffected.
    /// `sqlx(default)` guards the rare explicit-column query.
    #[serde(default)]
    #[sqlx(default)]
    pub kind: String,
    /// Soft "淡化/封存" marker (migration 0038). NULL = active. A
    /// state, not a type — deliberately separate from `kind`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[sqlx(default)]
    pub archived_at: Option<DateTime<Utc>>,
    /// Effective ACL role for the requesting user. Populated by list/get
    /// endpoints that join user_project_role(); NULL on INSERT…RETURNING paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[sqlx(default)]
    pub effective_role: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One source inside a multi-source workspace (migration 0040). A
/// workspace may aggregate many local folders and many Git repos; each
/// is a `project_sources` row. The legacy single columns on `projects`
/// remain the "primary" source for back-compat.
#[allow(dead_code)] // consumed by the multi-source indexer/freshen (MS-2)
#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct ProjectSource {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: String, // "local" | "git" | "upload"
    pub source_path: String,
    pub local_path: Option<String>,
    pub git_identity_id: Option<Uuid>,
    pub default_branch: Option<String>,
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    pub owner_user_id: Uuid,
    pub role: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct Workspace {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub role: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct GitIdentity {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub provider: String,
    pub username: String,
    #[serde(skip_serializing)]
    pub access_token: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct AgentProfile {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub provider: String,
    pub model: String,
    pub base_url: Option<String>,
    pub role_prompt: String,
    #[serde(skip_serializing)]
    pub api_key: String,
    pub enabled: bool,
    /// B7: free-form labels for grouping in the /agents page.
    pub labels: Vec<String>,
    pub allowed_classification_max: String,
    pub allow_code_context: bool,
    pub allow_project_memory: bool,
    pub allow_conversation_history: bool,
    pub require_redaction: bool,
    pub external_processing_allowed: bool,
    pub retention_policy: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct Conversation {
    pub id: Uuid,
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub title: String,
    pub mode: String, // "openclaw" | "hermes" | "debate" | "agent:<profile-id>" | "agents:<id1>,<id2>,..."
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct Message {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub role: String, // "user" | "hermes" | "openclaw" | "system"
    pub content: String,
    pub agent_name: Option<String>,
    pub file_path: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Author user id for messages where `role = 'user'`. NULL on assistant
    /// and system messages. Added by migration 0021 to support shared
    /// conversations where multiple collaborators post into the same thread.
    #[serde(default)]
    pub user_id: Option<Uuid>,
    /// Optional display name, populated by SELECTs that JOIN users.id.
    /// Not stored in the DB — leave as None for INSERT...RETURNING paths
    /// and the frontend will fall back to "You" for the current viewer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[sqlx(default)]
    pub author_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct ProjectMemorySummary {
    pub project_id: Uuid,
    pub summary: String,
    pub source_message_count: i32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct ConversationSummary {
    pub conversation_id: Uuid,
    pub summary: String,
    pub highlights: serde_json::Value,
    pub keywords: Vec<String>,
    pub source_message_count: i32,
    pub updated_at: DateTime<Utc>,
}

// ── Vault models (Phase 1 / Option B: User KEK + System Recovery) ────

/// Metadata row for a user's credential vault entry.
/// The actual secret lives in `vault_ciphertexts` with
/// `object_type = "vault_secret"` and `object_id = VaultSecret.id`.
#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct VaultSecret {
    pub id: Uuid,
    pub user_id: Uuid,
    pub label: String,
    pub secret_type: String,
    pub username: Option<String>,
    pub url: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One row in the immutable `vault_audit_log` table.
#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct VaultAuditEntry {
    pub id: Uuid,
    pub actor_id: Option<Uuid>,
    pub object_type: String,
    pub object_id: Uuid,
    pub operation: String,
    /// `"user"` = normal User KEK path.
    /// `"system_recovery"` = admin used System KEK (high-visibility event).
    pub kek_path: String,
    pub reason: String,
    pub ip_addr: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
#[allow(dead_code)]
pub struct RefreshToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}
