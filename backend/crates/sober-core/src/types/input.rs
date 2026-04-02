//! Input types for repository create/update operations.
//!
//! These are passed to repo trait methods. They contain only the fields
//! the caller provides — IDs, timestamps, and defaults are set by the
//! repo implementation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::{
    ArtifactKind, ArtifactState, EvolutionStatus, EvolutionType, PlatformType, PluginKind,
    PluginOrigin, PluginScope, PluginStatus,
};
use super::ids::{ArtifactId, ConversationId, PluginId, UserId, WorkspaceId};

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
    /// Message content blocks.
    pub content: Vec<super::content::ContentBlock>,
    /// LLM reasoning/thinking content.
    pub reasoning: Option<String>,
    /// Approximate token count.
    pub token_count: Option<i32>,
    /// Extensible metadata (e.g. event details).
    pub metadata: Option<serde_json::Value>,
    /// The user who sent this message (for user role messages).
    pub user_id: Option<super::ids::UserId>,
}

/// Input for creating a conversation attachment.
#[derive(Debug, Clone)]
pub struct CreateConversationAttachment {
    /// Content-addressed blob storage key.
    pub blob_key: String,
    /// Derived attachment kind.
    pub kind: super::enums::AttachmentKind,
    /// MIME content type.
    pub content_type: String,
    /// Original filename.
    pub filename: String,
    /// File size in bytes.
    pub size: i64,
    /// Type-specific metadata.
    pub metadata: serde_json::Value,
    /// The conversation this attachment belongs to.
    pub conversation_id: super::ids::ConversationId,
    /// The user who uploaded this attachment.
    pub user_id: super::ids::UserId,
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
    /// Who owns this job: "system", "user", or "group".
    pub owner_type: String,
    /// Owner UUID (None for system jobs).
    pub owner_id: Option<uuid::Uuid>,
    /// Workspace context for execution (None for system jobs).
    pub workspace_id: Option<uuid::Uuid>,
    /// User who created the job (None for system jobs).
    pub created_by: Option<uuid::Uuid>,
    /// Conversation to deliver results to (None for system/CLI-created jobs).
    pub conversation_id: Option<uuid::Uuid>,
    /// When the job should first run.
    pub next_run_at: DateTime<Utc>,
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
    /// Owning user.
    pub user_id: UserId,
    /// Conversation this secret is scoped to, if any.
    pub conversation_id: Option<ConversationId>,
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

/// Filter parameters for listing conversations.
#[derive(Debug, Clone, Default)]
pub struct ListConversationsFilter {
    /// Filter by archived status.
    pub archived: Option<bool>,
    /// Filter by conversation kind.
    pub kind: Option<super::enums::ConversationKind>,
    /// Filter by tag name.
    pub tag: Option<String>,
    /// Search by title (ILIKE).
    pub search: Option<String>,
}

/// Input for creating a new tag (idempotent).
///
/// Color is assigned deterministically by the repository based on the tag name.
#[derive(Debug, Clone)]
pub struct CreateTag {
    /// The user who owns the tag.
    pub user_id: UserId,
    /// Tag name.
    pub name: String,
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

/// Input for creating a new plugin.
#[derive(Debug, Clone)]
pub struct CreatePlugin {
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
    /// Initial lifecycle status.
    pub status: PluginStatus,
    /// Plugin-specific configuration (JSON).
    pub config: serde_json::Value,
    /// The user who installed this plugin.
    pub installed_by: Option<UserId>,
}

/// Input for creating a plugin audit log entry.
#[derive(Debug, Clone)]
pub struct CreatePluginAuditLog {
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
    /// The user who triggered the audit (None for agent-initiated).
    pub audited_by: Option<UserId>,
}

/// Input for creating a sandbox execution log.
#[derive(Debug, Clone)]
pub struct CreateSandboxExecutionLog {
    /// Unique execution identifier (from the sandbox audit entry).
    pub execution_id: uuid::Uuid,
    /// Workspace context (if applicable).
    pub workspace_id: Option<WorkspaceId>,
    /// User who triggered the execution (if applicable).
    pub user_id: Option<UserId>,
    /// Name of the sandbox policy used.
    pub policy_name: String,
    /// The command that was executed.
    pub command: Vec<String>,
    /// What triggered this execution (e.g. "agent", "scheduler", "user").
    pub trigger: String,
    /// Execution duration in milliseconds.
    pub duration_ms: i64,
    /// Process exit code (if completed).
    pub exit_code: Option<i32>,
    /// Domains denied by the network proxy.
    pub denied_network_requests: Vec<String>,
    /// How the execution ended (e.g. "success", "timeout", "error").
    pub outcome: String,
}

/// Input for creating a plugin invocation log.
#[derive(Debug, Clone)]
pub struct CreatePluginExecutionLog {
    /// The plugin that was invoked (if known).
    pub plugin_id: Option<PluginId>,
    /// Plugin name.
    pub plugin_name: String,
    /// Name of the tool that was called.
    pub tool_name: String,
    /// User who triggered the execution.
    pub user_id: Option<UserId>,
    /// Conversation context.
    pub conversation_id: Option<ConversationId>,
    /// Workspace context.
    pub workspace_id: Option<WorkspaceId>,
    /// Execution duration in milliseconds.
    pub duration_ms: i64,
    /// Whether the execution succeeded.
    pub success: bool,
    /// Error message if the execution failed.
    pub error_message: Option<String>,
}

/// Filter for querying plugins.
#[derive(Debug, Clone, Default)]
pub struct PluginFilter {
    /// Filter by name (exact match).
    pub name: Option<String>,
    /// Filter by plugin kind.
    pub kind: Option<PluginKind>,
    /// Filter by scope.
    pub scope: Option<PluginScope>,
    /// Filter by owner.
    pub owner_id: Option<UserId>,
    /// Filter by workspace.
    pub workspace_id: Option<WorkspaceId>,
    /// Filter by status.
    pub status: Option<PluginStatus>,
}

/// Input for creating a new platform connection.
#[derive(Debug, Deserialize)]
pub struct CreatePlatform {
    /// The platform type (Discord, Telegram, etc.).
    pub platform_type: PlatformType,
    /// Human-readable display name.
    pub display_name: String,
}

/// Input for updating a platform connection.
#[derive(Debug, Deserialize)]
pub struct UpdatePlatform {
    /// New display name.
    pub display_name: Option<String>,
    /// Whether the platform is enabled.
    pub is_enabled: Option<bool>,
}

/// Input for creating a channel mapping.
#[derive(Debug, Deserialize)]
pub struct CreateChannelMapping {
    /// External channel identifier (platform-specific).
    pub external_channel_id: String,
    /// Human-readable channel name.
    pub external_channel_name: String,
    /// The Sõber conversation to route messages to.
    pub conversation_id: ConversationId,
}

/// Input for creating a user mapping.
#[derive(Debug, Deserialize)]
pub struct CreateUserMapping {
    /// External user identifier (platform-specific).
    pub external_user_id: String,
    /// External username at time of mapping.
    pub external_username: String,
    /// The Sõber user to map to.
    pub user_id: UserId,
}

/// Input for creating an evolution event.
#[derive(Debug, Clone)]
pub struct CreateEvolutionEvent {
    /// Type of evolution.
    pub evolution_type: EvolutionType,
    /// Human-readable title.
    pub title: String,
    /// Agent's reasoning and evidence.
    pub description: String,
    /// Confidence score.
    pub confidence: f32,
    /// Source conversation count.
    pub source_count: i32,
    /// Initial status (proposed or approved for auto-approve).
    pub status: EvolutionStatus,
    /// Type-specific payload.
    pub payload: serde_json::Value,
    /// Whose patterns triggered this.
    pub user_id: Option<UserId>,
}
