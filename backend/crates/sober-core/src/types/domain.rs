//! Domain entity types used across all crates.
//!
//! These are the canonical representations of database entities. They are
//! returned by repo trait methods and consumed by business logic. Row types
//! (`FromRow` structs) are private to `sober-db` and convert into these.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::{
    ArtifactKind, ArtifactState, JobStatus, MessageRole, UserStatus, WorkspaceState, WorktreeState,
};
use super::ids::{
    ArtifactId, AuditLogId, ConversationId, EncryptionKeyId, JobId, JobRunId, McpServerId,
    MessageId, RoleId, ScopeId, SecretId, SessionId, UserId, WorkspaceId, WorkspaceRepoId,
    WorktreeId,
};

/// A user account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// Unique identifier.
    pub id: UserId,
    /// Email address (unique).
    pub email: String,
    /// Display username (unique).
    pub username: String,
    /// Account lifecycle status.
    pub status: UserStatus,
    /// When the account was created.
    pub created_at: DateTime<Utc>,
    /// When the account was last updated.
    pub updated_at: DateTime<Utc>,
}

/// An authorization role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    /// Unique identifier.
    pub id: RoleId,
    /// Role name (unique, e.g. "user", "admin").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// When the role was created.
    pub created_at: DateTime<Utc>,
}

/// A role assigned to a user, optionally scoped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRole {
    /// The user who holds this role.
    pub user_id: UserId,
    /// The role being granted.
    pub role_id: RoleId,
    /// Scope of the grant. Nil UUID means global.
    pub scope_id: ScopeId,
    /// Who granted this role (if known).
    pub granted_by: Option<UserId>,
    /// When the role was granted.
    pub granted_at: DateTime<Utc>,
}

/// An active user session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique identifier.
    pub id: SessionId,
    /// The user who owns this session.
    pub user_id: UserId,
    /// SHA-256 hash of the session token.
    pub token_hash: String,
    /// When the session expires.
    pub expires_at: DateTime<Utc>,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
}

/// A chat conversation owned by a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    /// Unique identifier.
    pub id: ConversationId,
    /// The user who owns this conversation.
    pub user_id: UserId,
    /// Optional conversation title.
    pub title: Option<String>,
    /// The workspace this conversation is scoped to, if any.
    pub workspace_id: Option<WorkspaceId>,
    /// Shell execution permission mode for this conversation.
    pub permission_mode: crate::PermissionMode,
    /// When the conversation was created.
    pub created_at: DateTime<Utc>,
    /// When the conversation was last updated.
    pub updated_at: DateTime<Utc>,
}

/// A message within a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Unique identifier.
    pub id: MessageId,
    /// The conversation this message belongs to.
    pub conversation_id: ConversationId,
    /// Author type (user, assistant, system, tool).
    pub role: MessageRole,
    /// Message content.
    pub content: String,
    /// Tool call requests (JSON), if this is an assistant message with tool use.
    pub tool_calls: Option<serde_json::Value>,
    /// Tool execution result (JSON), if this is a tool response.
    pub tool_result: Option<serde_json::Value>,
    /// Approximate token count for context budgeting.
    pub token_count: Option<i32>,
    /// When the message was created.
    pub created_at: DateTime<Utc>,
}

/// A per-user MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Unique identifier.
    pub id: McpServerId,
    /// The user who owns this configuration.
    pub user_id: UserId,
    /// Display name for the MCP server.
    pub name: String,
    /// Command to start the server.
    pub command: String,
    /// Command-line arguments.
    pub args: serde_json::Value,
    /// Environment variables.
    pub env: serde_json::Value,
    /// Whether the server is enabled.
    pub enabled: bool,
    /// When the configuration was created.
    pub created_at: DateTime<Utc>,
    /// When the configuration was last updated.
    pub updated_at: DateTime<Utc>,
}

/// A scheduled job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// Unique identifier.
    pub id: JobId,
    /// Human-readable job name.
    pub name: String,
    /// Cron expression or interval description.
    pub schedule: String,
    /// Job lifecycle status.
    pub status: JobStatus,
    /// The job payload (JSON) — defines what to execute.
    pub payload: serde_json::Value,
    /// Opaque binary payload for the scheduler.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub payload_bytes: Vec<u8>,
    /// Who owns this job: "system", "user", or "agent".
    #[serde(default = "default_owner_type")]
    pub owner_type: String,
    /// Owner UUID (None for system jobs).
    pub owner_id: Option<uuid::Uuid>,
    /// Whether to wake the agent when this job completes.
    #[serde(default)]
    pub notify_agent: bool,
    /// When the job should next run.
    pub next_run_at: DateTime<Utc>,
    /// When the job last ran.
    pub last_run_at: Option<DateTime<Utc>>,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
}

fn default_owner_type() -> String {
    "system".to_owned()
}

/// A record of a single job execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRun {
    /// Unique run identifier.
    pub id: JobRunId,
    /// Which job this run belongs to.
    pub job_id: JobId,
    /// When execution started.
    pub started_at: DateTime<Utc>,
    /// When execution finished (None if still running).
    pub finished_at: Option<DateTime<Utc>>,
    /// Run status: "running", "succeeded", or "failed".
    pub status: String,
    /// Result payload (empty if not yet finished).
    pub result: Vec<u8>,
    /// Error message (None if succeeded or still running).
    pub error: Option<String>,
}

/// A workspace containing related projects and artifacts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    /// Unique identifier.
    pub id: WorkspaceId,
    /// The user who owns this workspace.
    pub user_id: UserId,
    /// Display name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Root filesystem path.
    pub root_path: String,
    /// Lifecycle state.
    pub state: WorkspaceState,
    /// Who created this workspace.
    pub created_by: UserId,
    /// When the workspace was archived.
    pub archived_at: Option<DateTime<Utc>>,
    /// When the workspace was deleted.
    pub deleted_at: Option<DateTime<Utc>>,
    /// When the workspace was created.
    pub created_at: DateTime<Utc>,
    /// When the workspace was last updated.
    pub updated_at: DateTime<Utc>,
}

/// A git repository registered within a workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRepoEntry {
    /// Unique identifier.
    pub id: WorkspaceRepoId,
    /// The workspace this repo belongs to.
    pub workspace_id: WorkspaceId,
    /// Display name.
    pub name: String,
    /// Filesystem path to the repository.
    pub path: String,
    /// Whether this is a linked (external) repo vs managed.
    pub is_linked: bool,
    /// Remote URL (e.g., GitHub URL).
    pub remote_url: Option<String>,
    /// Default branch name.
    pub default_branch: String,
    /// When the repo was registered.
    pub created_at: DateTime<Utc>,
}

/// A git worktree linked to a workspace repo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    /// Unique identifier.
    pub id: WorktreeId,
    /// The repo this worktree belongs to.
    pub repo_id: WorkspaceRepoId,
    /// Branch name.
    pub branch: String,
    /// Filesystem path to the worktree.
    pub path: String,
    /// Lifecycle state.
    pub state: WorktreeState,
    /// Who created this worktree.
    pub created_by: Option<UserId>,
    /// Associated task ID (if any).
    pub task_id: Option<uuid::Uuid>,
    /// Associated conversation.
    pub conversation_id: Option<ConversationId>,
    /// When the worktree was created.
    pub created_at: DateTime<Utc>,
    /// When the worktree was last active.
    pub last_active_at: DateTime<Utc>,
}

/// A workspace artifact (code change, document, proposal, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// Unique identifier.
    pub id: ArtifactId,
    /// The workspace this artifact belongs to.
    pub workspace_id: WorkspaceId,
    /// The user who owns this artifact.
    pub user_id: UserId,
    /// Artifact kind.
    pub kind: ArtifactKind,
    /// Lifecycle state.
    pub state: ArtifactState,
    /// Display title.
    pub title: String,
    /// Optional description.
    pub description: Option<String>,
    /// Storage type: "git", "blob", or "inline".
    pub storage_type: String,
    /// Git repo path (if storage_type is "git").
    pub git_repo: Option<String>,
    /// Git ref (if storage_type is "git").
    pub git_ref: Option<String>,
    /// Blob key (if storage_type is "blob").
    pub blob_key: Option<String>,
    /// Inline content (if storage_type is "inline").
    pub inline_content: Option<String>,
    /// Who created this artifact (None = agent).
    pub created_by: Option<UserId>,
    /// Associated conversation.
    pub conversation_id: Option<ConversationId>,
    /// Associated task.
    pub task_id: Option<uuid::Uuid>,
    /// Parent artifact.
    pub parent_id: Option<ArtifactId>,
    /// Who reviewed this artifact.
    pub reviewed_by: Option<UserId>,
    /// When the artifact was reviewed.
    pub reviewed_at: Option<DateTime<Utc>>,
    /// Extensible metadata.
    pub metadata: serde_json::Value,
    /// When the artifact was created.
    pub created_at: DateTime<Utc>,
    /// When the artifact was last updated.
    pub updated_at: DateTime<Utc>,
}

/// An entry in the append-only audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    /// Unique identifier.
    pub id: AuditLogId,
    /// The user who performed the action (if known).
    pub actor_id: Option<UserId>,
    /// Action name (e.g. "user.create", "session.delete").
    pub action: String,
    /// Target entity type (e.g. "user", "conversation").
    pub target_type: Option<String>,
    /// Target entity ID.
    pub target_id: Option<uuid::Uuid>,
    /// Additional details (JSON).
    pub details: Option<serde_json::Value>,
    /// IP address of the actor.
    pub ip_address: Option<String>,
    /// When the action occurred.
    pub created_at: DateTime<Utc>,
}

/// Determines whether a secret or encryption key is scoped to a user.
///
/// Future: extend with `Group(GroupId)` when groups are implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretScope {
    /// Secret scoped to an individual user.
    User(UserId),
}

/// Metadata returned when listing secrets (no encrypted data included).
#[derive(Debug, Clone, Serialize)]
pub struct SecretMetadata {
    /// Unique identifier.
    pub id: SecretId,
    /// Human-readable label.
    pub name: String,
    /// Category (e.g. `"llm_provider"`, `"oauth_app"`, `"api_token"`).
    pub secret_type: String,
    /// Non-sensitive metadata (JSON) — provider name, base URL, etc.
    pub metadata: serde_json::Value,
    /// Priority for ordered fallback chains (lower = higher priority).
    pub priority: Option<i32>,
    /// When the secret was created.
    pub created_at: DateTime<Utc>,
    /// When the secret was last updated.
    pub updated_at: DateTime<Utc>,
}

/// Full secret row including encrypted data (returned only on explicit fetch).
#[derive(Debug, Clone)]
pub struct SecretRow {
    /// Unique identifier.
    pub id: SecretId,
    /// Owning user.
    pub user_id: UserId,
    /// Human-readable label.
    pub name: String,
    /// Category.
    pub secret_type: String,
    /// Non-sensitive metadata (JSON).
    pub metadata: serde_json::Value,
    /// AES-256-GCM encrypted data (nonce || ciphertext).
    pub encrypted_data: Vec<u8>,
    /// Priority for ordering.
    pub priority: Option<i32>,
    /// When the secret was created.
    pub created_at: DateTime<Utc>,
    /// When the secret was last updated.
    pub updated_at: DateTime<Utc>,
}

/// A stored DEK (data encryption key) for a user scope.
#[derive(Debug, Clone)]
pub struct StoredDek {
    /// Unique identifier.
    pub id: EncryptionKeyId,
    /// Owning user.
    pub user_id: UserId,
    /// MEK-wrapped DEK bytes (nonce || ciphertext).
    pub encrypted_dek: Vec<u8>,
    /// Which MEK version was used to wrap this DEK.
    pub mek_version: i32,
    /// When the DEK was created.
    pub created_at: DateTime<Utc>,
    /// When the DEK was last rotated.
    pub rotated_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_serializes_correctly() {
        let user = User {
            id: UserId::new(),
            email: "test@example.com".into(),
            username: "testuser".into(),
            status: UserStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_value(&user).unwrap();
        assert_eq!(json["email"], "test@example.com");
        assert_eq!(json["status"], "active");
    }

    #[test]
    fn message_with_tool_calls() {
        let msg = Message {
            id: MessageId::new(),
            conversation_id: ConversationId::new(),
            role: MessageRole::Assistant,
            content: "Let me search for that.".into(),
            tool_calls: Some(
                serde_json::json!([{"name": "web_search", "input": {"query": "rust"}}]),
            ),
            tool_result: None,
            token_count: Some(42),
            created_at: Utc::now(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert!(json["tool_calls"].is_array());
        assert_eq!(json["token_count"], 42);
    }

    #[test]
    fn job_serializes_correctly() {
        let job = Job {
            id: JobId::new(),
            name: "memory_prune".into(),
            schedule: "0 */6 * * *".into(),
            status: JobStatus::Active,
            payload: serde_json::json!({"scope": "all"}),
            payload_bytes: vec![],
            owner_type: "system".into(),
            owner_id: None,
            notify_agent: false,
            next_run_at: Utc::now(),
            last_run_at: None,
            created_at: Utc::now(),
        };
        let json = serde_json::to_value(&job).unwrap();
        assert_eq!(json["status"], "active");
        assert_eq!(json["name"], "memory_prune");
    }
}
