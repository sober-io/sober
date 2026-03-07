//! Repository trait definitions for database access.
//!
//! Traits are defined here in `sober-core` so library crates can depend on
//! them without importing `sqlx`. Concrete PostgreSQL implementations live
//! in `sober-db`.
//!
//! Uses Rust 2024 RPITIT — no `async_trait` crate needed.

use chrono::{DateTime, Utc};

use super::domain::*;
use super::enums::{ArtifactRelation, ArtifactState, UserStatus};
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

    /// Creates a new user and assigns them a role in a single transaction.
    fn create_with_role(
        &self,
        input: CreateUser,
        role: &str,
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

    /// Deletes a conversation and all its messages.
    fn delete(&self, id: ConversationId) -> impl Future<Output = Result<(), AppError>> + Send;
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
        root_path: &str,
    ) -> impl Future<Output = Result<Workspace, AppError>> + Send;

    /// Finds a workspace by ID.
    fn get_by_id(
        &self,
        id: WorkspaceId,
    ) -> impl Future<Output = Result<Workspace, AppError>> + Send;

    /// Lists all workspaces for a user.
    fn list_by_user(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<Workspace>, AppError>> + Send;

    /// Archives a workspace.
    fn archive(&self, id: WorkspaceId) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Restores an archived workspace.
    fn restore(&self, id: WorkspaceId) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Deletes a workspace.
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

    /// Finds a repo by filesystem path and user.
    fn find_by_path(
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
    ) -> impl Future<Output = Result<Worktree, AppError>> + Send;

    /// Lists worktrees for a repo.
    fn list_by_repo(
        &self,
        repo_id: WorkspaceRepoId,
    ) -> impl Future<Output = Result<Vec<Worktree>, AppError>> + Send;

    /// Lists worktrees created before the given time.
    fn list_stale(
        &self,
        older_than: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<Worktree>, AppError>> + Send;

    /// Marks a worktree as stale.
    fn mark_stale(&self, id: WorktreeId) -> impl Future<Output = Result<(), AppError>> + Send;

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

    /// Updates the state of an artifact.
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
