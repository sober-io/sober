//! Domain entity types used across all crates.
//!
//! These are the canonical representations of database entities. They are
//! returned by repo trait methods and consumed by business logic. Row types
//! (`FromRow` structs) are private to `sober-db` and convert into these.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::{
    AgentMode, ArtifactKind, ArtifactState, ConversationKind, ConversationUserRole, JobStatus,
    MessageRole, PluginKind, PluginOrigin, PluginScope, PluginStatus, UserStatus, WorkspaceState,
    WorktreeState,
};
use super::ids::{
    ArtifactId, AuditLogId, ConversationId, EncryptionKeyId, JobId, JobRunId, MessageId, PluginId,
    RoleId, ScopeId, SecretId, SessionId, TagId, UserId, WorkspaceId, WorkspaceRepoId, WorktreeId,
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
    /// The kind of conversation.
    pub kind: ConversationKind,
    /// Agent response mode for this conversation.
    pub agent_mode: AgentMode,
    /// Whether the conversation is archived.
    pub is_archived: bool,
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
    /// The user who sent this message (None for assistant/system/tool messages).
    pub user_id: Option<UserId>,
    /// Extensible metadata (e.g. event details for event-role messages).
    pub metadata: Option<serde_json::Value>,
    /// When the message was created.
    pub created_at: DateTime<Utc>,
}

/// A user's membership in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationUser {
    /// The conversation.
    pub conversation_id: ConversationId,
    /// The user.
    pub user_id: UserId,
    /// Number of unread messages.
    pub unread_count: i32,
    /// When the user last read this conversation.
    pub last_read_at: Option<DateTime<Utc>>,
    /// The user's role in this conversation.
    pub role: ConversationUserRole,
    /// When the user joined.
    pub joined_at: DateTime<Utc>,
}

/// A conversation member with their username (for display in member lists).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationUserWithUsername {
    /// The conversation.
    pub conversation_id: ConversationId,
    /// The user.
    pub user_id: UserId,
    /// The user's display username.
    pub username: String,
    /// Number of unread messages.
    pub unread_count: i32,
    /// When the user last read this conversation.
    pub last_read_at: Option<DateTime<Utc>>,
    /// The user's role in this conversation.
    pub role: ConversationUserRole,
    /// When the user joined.
    pub joined_at: DateTime<Utc>,
}

/// A user-created tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    /// Unique identifier.
    pub id: TagId,
    /// The user who owns this tag.
    pub user_id: UserId,
    /// Tag display name.
    pub name: String,
    /// Hex color code.
    pub color: String,
    /// When the tag was created.
    pub created_at: DateTime<Utc>,
}

/// A conversation with additional details for list/detail views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationWithDetails {
    /// The base conversation.
    #[serde(flatten)]
    pub conversation: Conversation,
    /// Number of unread messages for the requesting user.
    pub unread_count: i32,
    /// Tags applied to this conversation.
    pub tags: Vec<Tag>,
    /// Users in this conversation (populated for detail view, empty for list view).
    pub users: Vec<ConversationUser>,
    /// Linked workspace name (joined from workspaces table).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
    /// Linked workspace root path (joined from workspaces table).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
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
    /// Who owns this job: "system", "user", or "group".
    #[serde(default = "default_owner_type")]
    pub owner_type: String,
    /// Owner UUID (None for system jobs).
    pub owner_id: Option<uuid::Uuid>,
    /// Workspace context for execution (None for system jobs).
    pub workspace_id: Option<uuid::Uuid>,
    /// User who created the job (None for system jobs).
    pub created_by: Option<uuid::Uuid>,
    /// Conversation to deliver results to (None for system/soberctl jobs).
    pub conversation_id: Option<uuid::Uuid>,
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
    /// Conversation this secret is scoped to, if any.
    pub conversation_id: Option<ConversationId>,
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
    /// Conversation this secret is scoped to, if any.
    pub conversation_id: Option<ConversationId>,
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

/// A registered plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    /// Unique identifier.
    pub id: PluginId,
    /// Display name.
    pub name: String,
    /// Plugin kind (MCP, Skill, WASM).
    pub kind: PluginKind,
    /// Semantic version string.
    pub version: Option<String>,
    /// Human-readable description.
    pub description: Option<String>,
    /// How this plugin was discovered/installed.
    pub origin: PluginOrigin,
    /// Availability scope (system, user, workspace).
    pub scope: PluginScope,
    /// The user this plugin belongs to (for user-scoped plugins).
    pub owner_id: Option<UserId>,
    /// The workspace this plugin belongs to (for workspace-scoped plugins).
    pub workspace_id: Option<WorkspaceId>,
    /// Lifecycle status.
    pub status: PluginStatus,
    /// Plugin-specific configuration (JSON).
    pub config: serde_json::Value,
    /// The user who installed this plugin.
    pub installed_by: Option<UserId>,
    /// When the plugin was installed.
    pub installed_at: DateTime<Utc>,
    /// When the plugin was last updated.
    pub updated_at: DateTime<Utc>,
}

/// An audit log entry for a plugin security audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAuditLog {
    /// Unique identifier.
    pub id: uuid::Uuid,
    /// The plugin that was audited (None if rejected before creation).
    pub plugin_id: Option<PluginId>,
    /// Plugin name at the time of audit.
    pub plugin_name: String,
    /// Plugin kind at the time of audit.
    pub kind: PluginKind,
    /// Plugin origin at the time of audit.
    pub origin: PluginOrigin,
    /// Audit pipeline stages and their results (JSON).
    pub stages: serde_json::Value,
    /// Overall verdict (e.g. "approved", "rejected").
    pub verdict: String,
    /// Reason for rejection, if applicable.
    pub rejection_reason: Option<String>,
    /// When the audit was performed.
    pub audited_at: DateTime<Utc>,
    /// The user who triggered the audit (None for agent-initiated).
    pub audited_by: Option<UserId>,
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
            user_id: None,
            metadata: None,
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
            owner_type: "system".into(),
            owner_id: None,
            workspace_id: None,
            created_by: None,
            conversation_id: None,
            next_run_at: Utc::now(),
            last_run_at: None,
            created_at: Utc::now(),
        };
        let json = serde_json::to_value(&job).unwrap();
        assert_eq!(json["status"], "active");
        assert_eq!(json["name"], "memory_prune");
    }
}
