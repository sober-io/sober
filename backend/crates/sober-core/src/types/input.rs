//! Input types for repository create/update operations.
//!
//! These are passed to repo trait methods. They contain only the fields
//! the caller provides — IDs, timestamps, and defaults are set by the
//! repo implementation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::domain::SecretScope;
use super::enums::{ArtifactKind, ArtifactState};
use super::ids::{ArtifactId, ConversationId, UserId, WorkspaceId};

/// Input for creating a new user.
#[derive(Debug, Clone)]
pub struct CreateUser {
    /// Email address.
    pub email: String,
    /// Display username.
    pub username: String,
    /// Argon2id password hash.
    pub password_hash: String,
}

/// Input for creating a new session.
#[derive(Debug, Clone)]
pub struct CreateSession {
    /// The user who owns this session.
    pub user_id: UserId,
    /// SHA-256 hash of the session token.
    pub token_hash: String,
    /// When the session expires.
    pub expires_at: DateTime<Utc>,
}

/// Input for creating a new message.
#[derive(Debug, Clone)]
pub struct CreateMessage {
    /// The conversation to add the message to.
    pub conversation_id: ConversationId,
    /// Author type.
    pub role: super::enums::MessageRole,
    /// Message content.
    pub content: String,
    /// Tool call requests (JSON).
    pub tool_calls: Option<serde_json::Value>,
    /// Tool execution result (JSON).
    pub tool_result: Option<serde_json::Value>,
    /// Approximate token count.
    pub token_count: Option<i32>,
}

/// Input for creating a new scheduled job.
#[derive(Debug, Clone)]
pub struct CreateJob {
    /// Human-readable job name.
    pub name: String,
    /// Cron expression or interval description.
    pub schedule: String,
    /// Job payload (JSON).
    pub payload: serde_json::Value,
    /// When the job should first run.
    pub next_run_at: Option<DateTime<Utc>>,
}

/// Input for creating an MCP server configuration.
#[derive(Debug, Clone)]
pub struct CreateMcpServer {
    /// The user who owns this configuration.
    pub user_id: UserId,
    /// Display name.
    pub name: String,
    /// Command to start the server.
    pub command: String,
    /// Command-line arguments (JSON array).
    pub args: serde_json::Value,
    /// Environment variables (JSON object).
    pub env: serde_json::Value,
}

/// Input for updating an MCP server configuration.
#[derive(Debug, Clone, Default)]
pub struct UpdateMcpServer {
    /// New display name.
    pub name: Option<String>,
    /// New command.
    pub command: Option<String>,
    /// New arguments.
    pub args: Option<serde_json::Value>,
    /// New environment variables.
    pub env: Option<serde_json::Value>,
    /// New enabled state.
    pub enabled: Option<bool>,
}

/// Input for registering a git repository in a workspace.
#[derive(Debug, Clone)]
pub struct RegisterRepo {
    /// Display name.
    pub name: String,
    /// Filesystem path.
    pub path: String,
    /// Whether this is a linked (external) repo.
    pub is_linked: bool,
    /// Remote URL.
    pub remote_url: Option<String>,
    /// Default branch name.
    pub default_branch: String,
}

/// Input for creating a workspace artifact.
#[derive(Debug, Clone, Default)]
pub struct CreateArtifact {
    /// The workspace this artifact belongs to.
    pub workspace_id: WorkspaceId,
    /// The user who owns this artifact.
    pub user_id: UserId,
    /// Artifact kind.
    pub kind: ArtifactKind,
    /// Display title.
    pub title: String,
    /// Optional description.
    pub description: Option<String>,
    /// Storage type: "git", "blob", or "inline".
    pub storage_type: String,
    /// Git repo path within workspace (if git).
    pub git_repo: Option<String>,
    /// Git ref (if git).
    pub git_ref: Option<String>,
    /// Blob key (if blob).
    pub blob_key: Option<String>,
    /// Inline content (if inline).
    pub inline_content: Option<String>,
    /// Who created this artifact (None = agent).
    pub created_by: Option<UserId>,
    /// Associated conversation.
    pub conversation_id: Option<ConversationId>,
    /// Associated task.
    pub task_id: Option<uuid::Uuid>,
    /// Parent artifact.
    pub parent_id: Option<ArtifactId>,
}

/// Filter for querying artifacts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArtifactFilter {
    /// Filter by kind.
    pub kind: Option<ArtifactKind>,
    /// Filter by state.
    pub state: Option<ArtifactState>,
}

/// Input for creating an audit log entry.
#[derive(Debug, Clone)]
pub struct CreateAuditLog {
    /// The user who performed the action.
    pub actor_id: Option<UserId>,
    /// Action name.
    pub action: String,
    /// Target entity type.
    pub target_type: Option<String>,
    /// Target entity ID.
    pub target_id: Option<uuid::Uuid>,
    /// Additional details (JSON).
    pub details: Option<serde_json::Value>,
    /// IP address.
    pub ip_address: Option<String>,
}

/// Input for storing a new encrypted secret.
#[derive(Debug)]
pub struct NewSecret {
    /// Which scope (user) owns this secret.
    pub scope: SecretScope,
    /// Human-readable label.
    pub name: String,
    /// Category (e.g. `"llm_provider"`, `"oauth_app"`).
    pub secret_type: String,
    /// Non-sensitive metadata (JSON).
    pub metadata: serde_json::Value,
    /// AES-256-GCM encrypted data (nonce || ciphertext).
    pub encrypted_data: Vec<u8>,
    /// Priority for ordered fallback chains.
    pub priority: Option<i32>,
}

/// Input for updating an existing secret.
#[derive(Debug, Default)]
pub struct UpdateSecret {
    /// New label.
    pub name: Option<String>,
    /// New metadata.
    pub metadata: Option<serde_json::Value>,
    /// New encrypted data.
    pub encrypted_data: Option<Vec<u8>>,
    /// New priority (`Some(None)` clears the priority).
    pub priority: Option<Option<i32>>,
}
