//! Input types for repository create/update operations.
//!
//! These are passed to repo trait methods. They contain only the fields
//! the caller provides — IDs, timestamps, and defaults are set by the
//! repo implementation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::{ArtifactKind, ArtifactState};
use super::ids::{ConversationId, UserId, WorkspaceId};

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
    /// Default branch name.
    pub default_branch: String,
}

/// Input for creating a workspace artifact.
#[derive(Debug, Clone)]
pub struct CreateArtifact {
    /// The workspace this artifact belongs to.
    pub workspace_id: WorkspaceId,
    /// Display name.
    pub name: String,
    /// Artifact kind.
    pub kind: ArtifactKind,
    /// Filesystem path (relative to workspace root).
    pub path: String,
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
