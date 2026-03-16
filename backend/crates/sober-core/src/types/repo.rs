//! Repository trait definitions for database access.
//!
//! Traits are defined here in `sober-core` so library crates can depend on
//! them without importing `sqlx`. Concrete PostgreSQL implementations live
//! in `sober-db`.
//!
//! Uses Rust 2024 RPITIT — no `async_trait` crate needed.

use chrono::{DateTime, Utc};

use super::domain::*;
use super::enums::{
    AgentMode, ArtifactRelation, ArtifactState, ConversationUserRole, JobStatus, RoleKind,
    UserStatus,
};
use super::ids::*;
use super::input::*;
use crate::error::AppError;

/// User account operations.
pub trait UserRepo: Send + Sync {
    /// Finds a user by their unique ID.
    fn get_by_id(&self, id: UserId) -> impl Future<Output = Result<User, AppError>> + Send;

    /// Finds a user by their email address.
    fn get_by_email(&self, email: &str) -> impl Future<Output = Result<User, AppError>> + Send;

    /// Finds a user by their username.
    fn get_by_username(
        &self,
        username: &str,
    ) -> impl Future<Output = Result<User, AppError>> + Send;

    /// Creates a new user account.
    fn create(&self, input: CreateUser) -> impl Future<Output = Result<User, AppError>> + Send;

    /// Creates a new user and assigns them one or more roles in a single transaction.
    fn create_with_roles(
        &self,
        input: CreateUser,
        roles: &[RoleKind],
    ) -> impl Future<Output = Result<User, AppError>> + Send;

    /// Updates a user's account status.
    fn update_status(
        &self,
        id: UserId,
        status: UserStatus,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Returns the password hash for a user (for authentication).
    fn get_password_hash(
        &self,
        id: UserId,
    ) -> impl Future<Output = Result<String, AppError>> + Send;

    /// Updates the password hash for a user.
    fn update_password_hash(
        &self,
        id: UserId,
        password_hash: &str,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Lists users, optionally filtered by status.
    fn list(
        &self,
        status: Option<UserStatus>,
    ) -> impl Future<Output = Result<Vec<User>, AppError>> + Send;

    /// Searches active users whose username starts with the given query (prefix match).
    ///
    /// Results are ordered by username and limited to `limit` rows.
    fn search_by_username(
        &self,
        query: &str,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<User>, AppError>> + Send;
}

/// Session management operations.
pub trait SessionRepo: Send + Sync {
    /// Finds a session by its token hash.
    fn get_by_token_hash(
        &self,
        token_hash: &str,
    ) -> impl Future<Output = Result<Option<Session>, AppError>> + Send;

    /// Creates a new session.
    fn create(
        &self,
        input: CreateSession,
    ) -> impl Future<Output = Result<Session, AppError>> + Send;

    /// Deletes a session by its token hash.
    fn delete_by_token_hash(
        &self,
        token_hash: &str,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Deletes all expired sessions. Returns the number of rows removed.
    fn cleanup_expired(&self) -> impl Future<Output = Result<u64, AppError>> + Send;
}

/// Conversation operations.
pub trait ConversationRepo: Send + Sync {
    /// Creates a new conversation.
    fn create(
        &self,
        user_id: UserId,
        title: Option<&str>,
        workspace_id: Option<WorkspaceId>,
    ) -> impl Future<Output = Result<Conversation, AppError>> + Send;

    /// Creates a new group conversation with the given title.
    fn create_group(
        &self,
        user_id: UserId,
        title: &str,
        workspace_id: Option<WorkspaceId>,
    ) -> impl Future<Output = Result<Conversation, AppError>> + Send;

    /// Finds a conversation by ID.
    fn get_by_id(
        &self,
        id: ConversationId,
    ) -> impl Future<Output = Result<Conversation, AppError>> + Send;

    /// Lists all conversations for a user, ordered by most recent first.
    fn list_by_user(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<Conversation>, AppError>> + Send;

    /// Updates the title of a conversation.
    fn update_title(
        &self,
        id: ConversationId,
        title: &str,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Updates the permission mode of a conversation.
    fn update_permission_mode(
        &self,
        id: ConversationId,
        mode: crate::PermissionMode,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Deletes a conversation and all its messages.
    fn delete(&self, id: ConversationId) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Finds the most recent conversation for a user, optionally in a workspace.
    fn find_latest_by_user_and_workspace(
        &self,
        user_id: UserId,
        workspace_id: Option<WorkspaceId>,
    ) -> impl Future<Output = Result<Option<Conversation>, AppError>> + Send;

    /// Lists conversations for a user with filters, including unread counts and tags.
    fn list_with_details(
        &self,
        user_id: UserId,
        filter: ListConversationsFilter,
    ) -> impl Future<Output = Result<Vec<ConversationWithDetails>, AppError>> + Send;

    /// Gets the user's inbox conversation.
    fn get_inbox(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Conversation, AppError>> + Send;

    /// Creates an inbox conversation for a user.
    fn create_inbox(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Conversation, AppError>> + Send;

    /// Updates the archived status of a conversation.
    fn update_archived(
        &self,
        id: ConversationId,
        archived: bool,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Links or unlinks a workspace from a conversation.
    fn update_workspace(
        &self,
        id: ConversationId,
        workspace_id: Option<WorkspaceId>,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Updates the agent mode for a conversation.
    fn update_agent_mode(
        &self,
        id: ConversationId,
        agent_mode: AgentMode,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Converts a direct conversation to a group conversation.
    ///
    /// Fails if the conversation is not currently `direct`.
    fn convert_to_group(
        &self,
        id: ConversationId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;
}

/// Message operations.
pub trait MessageRepo: Send + Sync {
    /// Creates a new message in a conversation.
    fn create(
        &self,
        input: CreateMessage,
    ) -> impl Future<Output = Result<Message, AppError>> + Send;

    /// Lists messages in a conversation, ordered oldest-first, with a limit.
    fn list_by_conversation(
        &self,
        conversation_id: ConversationId,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<Message>, AppError>> + Send;

    /// Lists messages with cursor-based pagination (newest first before cursor).
    fn list_paginated(
        &self,
        conversation_id: ConversationId,
        before: Option<MessageId>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<Message>, AppError>> + Send;

    /// Deletes a single message by ID.
    fn delete(&self, id: MessageId) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Deletes all messages in a conversation.
    fn clear_conversation(
        &self,
        conversation_id: ConversationId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Gets a single message by ID.
    fn get_by_id(&self, id: MessageId) -> impl Future<Output = Result<Message, AppError>> + Send;
}

/// Conversation membership and unread tracking operations.
pub trait ConversationUserRepo: Send + Sync {
    /// Creates a membership row (e.g., when a conversation is created).
    fn create(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        role: ConversationUserRole,
    ) -> impl Future<Output = Result<ConversationUser, AppError>> + Send;

    /// Marks a conversation as read for a user (resets unread_count to 0).
    fn mark_read(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Increments unread_count for all users in a conversation except the sender.
    fn increment_unread(
        &self,
        conversation_id: ConversationId,
        exclude_user_id: UserId,
    ) -> impl Future<Output = Result<Vec<(UserId, i32)>, AppError>> + Send;

    /// Gets the membership row for a user in a conversation.
    fn get(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> impl Future<Output = Result<ConversationUser, AppError>> + Send;

    /// Lists all users in a conversation.
    fn list_by_conversation(
        &self,
        conversation_id: ConversationId,
    ) -> impl Future<Output = Result<Vec<ConversationUser>, AppError>> + Send;

    /// Resets unread_count to 0 for ALL users in a conversation (used by /clear).
    fn reset_all_unread(
        &self,
        conversation_id: ConversationId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Lists all members with usernames (joins users table).
    fn list_members(
        &self,
        conversation_id: ConversationId,
    ) -> impl Future<Output = Result<Vec<ConversationUserWithUsername>, AppError>> + Send;

    /// Updates a member's role.
    fn update_role(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        role: ConversationUserRole,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Removes a member from a conversation.
    fn remove_member(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;
}

/// Tag operations.
pub trait TagRepo: Send + Sync {
    /// Creates a tag (idempotent — returns existing if name matches).
    fn create_or_get(&self, input: CreateTag)
    -> impl Future<Output = Result<Tag, AppError>> + Send;

    /// Lists all tags for a user.
    fn list_by_user(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<Tag>, AppError>> + Send;

    /// Adds a tag to a conversation.
    fn tag_conversation(
        &self,
        conversation_id: ConversationId,
        tag_id: TagId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Removes a tag from a conversation.
    fn untag_conversation(
        &self,
        conversation_id: ConversationId,
        tag_id: TagId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Adds a tag to a message.
    fn tag_message(
        &self,
        message_id: MessageId,
        tag_id: TagId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Removes a tag from a message.
    fn untag_message(
        &self,
        message_id: MessageId,
        tag_id: TagId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Lists tags for a conversation.
    fn list_by_conversation(
        &self,
        conversation_id: ConversationId,
    ) -> impl Future<Output = Result<Vec<Tag>, AppError>> + Send;
}

/// Scheduled job operations.
pub trait JobRepo: Send + Sync {
    /// Creates a new scheduled job.
    fn create(&self, input: CreateJob) -> impl Future<Output = Result<Job, AppError>> + Send;

    /// Finds a job by ID.
    fn get_by_id(&self, id: JobId) -> impl Future<Output = Result<Job, AppError>> + Send;

    /// Lists all active (non-cancelled) jobs.
    fn list_active(&self) -> impl Future<Output = Result<Vec<Job>, AppError>> + Send;

    /// Updates the next run time for a job.
    fn update_next_run(
        &self,
        id: JobId,
        next_run_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Records that a job just ran.
    fn mark_last_run(
        &self,
        id: JobId,
        ran_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Cancels a job.
    fn cancel(&self, id: JobId) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Updates a job's status.
    fn update_status(
        &self,
        id: JobId,
        status: JobStatus,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Lists jobs that are due for execution (next_run_at <= now, status = active).
    fn list_due(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<Job>, AppError>> + Send;

    /// Lists jobs with optional filters.
    fn list_filtered(
        &self,
        owner_type: Option<&str>,
        owner_id: Option<uuid::Uuid>,
        statuses: &[String],
        workspace_id: Option<uuid::Uuid>,
        name_filter: Option<&str>,
        conversation_id: Option<uuid::Uuid>,
    ) -> impl Future<Output = Result<Vec<Job>, AppError>> + Send;
}

/// Job execution run tracking.
pub trait JobRunRepo: Send + Sync {
    /// Creates a new run record (status = running).
    fn create(&self, job_id: JobId) -> impl Future<Output = Result<JobRun, AppError>> + Send;

    /// Marks a run as completed (succeeded or failed).
    fn complete(
        &self,
        id: JobRunId,
        result: Vec<u8>,
        error: Option<String>,
        result_artifact_ref: Option<String>,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Lists recent runs for a job, ordered by started_at descending.
    fn list_by_job(
        &self,
        job_id: JobId,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<JobRun>, AppError>> + Send;
}

/// Per-user MCP server configuration operations.
pub trait McpServerRepo: Send + Sync {
    /// Lists all MCP server configs for a user.
    fn list_by_user(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<McpServerConfig>, AppError>> + Send;

    /// Creates a new MCP server configuration.
    fn create(
        &self,
        input: CreateMcpServer,
    ) -> impl Future<Output = Result<McpServerConfig, AppError>> + Send;

    /// Updates an MCP server configuration.
    fn update(
        &self,
        id: McpServerId,
        input: UpdateMcpServer,
    ) -> impl Future<Output = Result<McpServerConfig, AppError>> + Send;

    /// Deletes an MCP server configuration.
    fn delete(&self, id: McpServerId) -> impl Future<Output = Result<(), AppError>> + Send;
}

/// Workspace operations.
pub trait WorkspaceRepo: Send + Sync {
    /// Creates a new workspace.
    fn create(
        &self,
        user_id: UserId,
        name: &str,
        description: Option<&str>,
        root_path: &str,
    ) -> impl Future<Output = Result<Workspace, AppError>> + Send;

    /// Finds a workspace by ID.
    fn get_by_id(
        &self,
        id: WorkspaceId,
    ) -> impl Future<Output = Result<Workspace, AppError>> + Send;

    /// Lists non-deleted workspaces for a user.
    fn list_by_user(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<Workspace>, AppError>> + Send;

    /// Archives a workspace (active -> archived).
    fn archive(&self, id: WorkspaceId) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Restores an archived workspace (archived -> active).
    fn restore(&self, id: WorkspaceId) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Soft-deletes a workspace (archived -> deleted).
    fn delete(&self, id: WorkspaceId) -> impl Future<Output = Result<(), AppError>> + Send;
}

/// Workspace git repository operations.
pub trait WorkspaceRepoRepo: Send + Sync {
    /// Registers a git repository in a workspace.
    fn register(
        &self,
        workspace_id: WorkspaceId,
        input: RegisterRepo,
    ) -> impl Future<Output = Result<WorkspaceRepoEntry, AppError>> + Send;

    /// Lists all repos in a workspace.
    fn list_by_workspace(
        &self,
        workspace_id: WorkspaceId,
    ) -> impl Future<Output = Result<Vec<WorkspaceRepoEntry>, AppError>> + Send;

    /// Finds a linked repo by filesystem path and user.
    fn find_by_linked_path(
        &self,
        path: &str,
        user_id: UserId,
    ) -> impl Future<Output = Result<Option<WorkspaceRepoEntry>, AppError>> + Send;

    /// Deletes a registered repo.
    fn delete(&self, id: WorkspaceRepoId) -> impl Future<Output = Result<(), AppError>> + Send;
}

/// Worktree operations.
pub trait WorktreeRepo: Send + Sync {
    /// Creates a worktree record.
    fn create(
        &self,
        repo_id: WorkspaceRepoId,
        branch: &str,
        path: &str,
        created_by: Option<UserId>,
        task_id: Option<uuid::Uuid>,
        conversation_id: Option<ConversationId>,
    ) -> impl Future<Output = Result<Worktree, AppError>> + Send;

    /// Finds a worktree by ID.
    fn get_by_id(&self, id: WorktreeId) -> impl Future<Output = Result<Worktree, AppError>> + Send;

    /// Lists worktrees for a repo.
    fn list_by_repo(
        &self,
        repo_id: WorkspaceRepoId,
    ) -> impl Future<Output = Result<Vec<Worktree>, AppError>> + Send;

    /// Lists stale worktree candidates (active with last_active_at older than threshold).
    fn list_stale_candidates(
        &self,
        older_than: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<Worktree>, AppError>> + Send;

    /// Marks a worktree as stale (active -> stale).
    fn mark_stale(&self, id: WorktreeId) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Marks a worktree as removing (stale -> removing).
    fn mark_removing(&self, id: WorktreeId) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Deletes a worktree record.
    fn delete(&self, id: WorktreeId) -> impl Future<Output = Result<(), AppError>> + Send;
}

/// Artifact operations.
pub trait ArtifactRepo: Send + Sync {
    /// Creates a new artifact.
    fn create(
        &self,
        input: CreateArtifact,
    ) -> impl Future<Output = Result<Artifact, AppError>> + Send;

    /// Finds an artifact by ID.
    fn get_by_id(&self, id: ArtifactId) -> impl Future<Output = Result<Artifact, AppError>> + Send;

    /// Lists artifacts in a workspace with optional filters.
    fn list_by_workspace(
        &self,
        workspace_id: WorkspaceId,
        filter: ArtifactFilter,
    ) -> impl Future<Output = Result<Vec<Artifact>, AppError>> + Send;

    /// Lists artifacts visible to a user (filters traces for non-admins).
    fn list_visible(
        &self,
        workspace_id: WorkspaceId,
        is_admin: bool,
    ) -> impl Future<Output = Result<Vec<Artifact>, AppError>> + Send;

    /// Updates the state of an artifact (validates legal transitions).
    fn update_state(
        &self,
        id: ArtifactId,
        state: ArtifactState,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Adds a relationship between two artifacts.
    fn add_relation(
        &self,
        source: ArtifactId,
        target: ArtifactId,
        relation: ArtifactRelation,
    ) -> impl Future<Output = Result<(), AppError>> + Send;
}

/// Role assignment query operations.
pub trait RoleRepo: Send + Sync {
    /// Returns the roles assigned to a user in the global scope.
    fn get_roles_for_user(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<RoleKind>, AppError>> + Send;
}

/// Audit log operations (append-only).
pub trait AuditLogRepo: Send + Sync {
    /// Appends an entry to the audit log.
    fn create(
        &self,
        input: CreateAuditLog,
    ) -> impl Future<Output = Result<AuditLogEntry, AppError>> + Send;

    /// Lists recent audit log entries, newest first.
    fn list_recent(
        &self,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<AuditLogEntry>, AppError>> + Send;

    /// Lists audit log entries for a specific actor.
    fn list_by_actor(
        &self,
        actor_id: UserId,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<AuditLogEntry>, AppError>> + Send;
}

/// Encrypted secret storage operations.
///
/// Manages the two-layer key hierarchy: DEKs (data encryption keys) that
/// are wrapped by a MEK, and user secrets encrypted by DEKs.
pub trait SecretRepo: Send + Sync {
    /// Gets the encrypted DEK for a scope, if one exists.
    fn get_dek(
        &self,
        scope: SecretScope,
    ) -> impl Future<Output = Result<Option<StoredDek>, AppError>> + Send;

    /// Stores or replaces the encrypted DEK for a scope.
    fn store_dek(
        &self,
        scope: SecretScope,
        encrypted_dek: Vec<u8>,
        mek_version: i32,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Lists secret metadata (without encrypted data) for a scope.
    ///
    /// Results are ordered by priority (ascending, NULLs last).
    /// Optionally filters by `secret_type`.
    fn list_secrets(
        &self,
        scope: SecretScope,
        secret_type: Option<&str>,
    ) -> impl Future<Output = Result<Vec<SecretMetadata>, AppError>> + Send;

    /// Gets a single secret including its encrypted data.
    fn get_secret(
        &self,
        id: SecretId,
    ) -> impl Future<Output = Result<Option<SecretRow>, AppError>> + Send;

    /// Gets a single secret by name within a scope.
    fn get_secret_by_name(
        &self,
        scope: SecretScope,
        name: &str,
    ) -> impl Future<Output = Result<Option<SecretRow>, AppError>> + Send;

    /// Stores a new secret. Returns the generated ID.
    fn store_secret(
        &self,
        secret: NewSecret,
    ) -> impl Future<Output = Result<SecretId, AppError>> + Send;

    /// Updates an existing secret.
    fn update_secret(
        &self,
        id: SecretId,
        update: UpdateSecret,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Deletes a secret by ID.
    fn delete_secret(&self, id: SecretId) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Lists all secret IDs for a scope (for bulk operations like DEK rotation).
    fn list_secret_ids(
        &self,
        scope: SecretScope,
    ) -> impl Future<Output = Result<Vec<SecretId>, AppError>> + Send;
}
