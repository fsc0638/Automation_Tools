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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct Project {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub source_type: String,        // "local" | "git" | "upload"
    pub source_path: String,        // local path, git URL, or uploaded project path
    pub local_path: Option<String>, // cloned path for git repos
    pub default_branch: Option<String>,
    pub git_identity_id: Option<Uuid>,
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
    pub mode: String, // "openclaw" | "hermes" | "debate"
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
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct ProjectMemorySummary {
    pub project_id: Uuid,
    pub summary: String,
    pub source_message_count: i32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct RefreshToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}
