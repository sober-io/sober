# 005 — sober-db: Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build the centralized PostgreSQL access layer (`sober-db`) and add domain types + repo traits to `sober-core`.

**Architecture:** Repo traits and domain types live in `sober-core`. `sober-db` is the only crate that depends on `sqlx` — it owns pool creation, private row types, and all `Pg*Repo` implementations. Library crates program against traits; binaries wire in concrete repos at startup.

**Tech Stack:** Rust 2024 edition, sqlx 0.8 (compile-time checked queries), PostgreSQL 17, tokio, tracing

**References:**
- Design: `docs/plans/pending/005-sober-db/design.md`
- Schema: `docs/plans/active/003-sober-core/schema.md`
- Existing sober-core: `backend/crates/sober-core/src/`

---

## Phase 1: sober-core — Domain Types, Enums & IDs

### Task 1: Add new ID types to sober-core

**Files:**
- Modify: `backend/crates/sober-core/src/types/ids.rs`
- Modify: `backend/crates/sober-core/src/types/mod.rs`

**Step 1: Add new ID newtypes**

In `ids.rs`, add after the existing `WorkspaceId` definition:

```rust
define_id!(
    /// Unique identifier for a scheduled job.
    JobId
);

define_id!(
    /// Unique identifier for a git repository registered in a workspace.
    WorkspaceRepoId
);

define_id!(
    /// Unique identifier for a git worktree.
    WorktreeId
);

define_id!(
    /// Unique identifier for a workspace artifact.
    ArtifactId
);

define_id!(
    /// Unique identifier for an audit log entry.
    AuditLogId
);
```

**Step 2: Update mod.rs re-exports**

In `types/mod.rs`, update the `ids` use statement to include the new types:

```rust
pub use ids::{
    ArtifactId, AuditLogId, ConversationId, JobId, McpServerId, MessageId, RoleId,
    ScopeId, SessionId, ToolId, UserId, WorkspaceId, WorkspaceRepoId, WorktreeId,
};
```

**Step 3: Run tests**

Run: `cd backend && cargo test -p sober-core -q`
Expected: All existing tests pass, new IDs work via macro.

**Step 4: Commit**

```
feat(core): add JobId, WorkspaceRepoId, WorktreeId, ArtifactId, AuditLogId
```

---

### Task 2: Add new enums to sober-core

**Files:**
- Modify: `backend/crates/sober-core/src/types/enums.rs`
- Modify: `backend/crates/sober-core/src/types/mod.rs`

**Step 1: Add enums**

Append to `enums.rs` (before the `#[cfg(test)]` module):

```rust
/// Lifecycle state of a scheduled job.
///
/// Maps to the `job_status` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "job_status", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    /// Job is active and will run on schedule.
    Active,
    /// Job is temporarily paused.
    Paused,
    /// Job has been cancelled and will not run again.
    Cancelled,
}

/// Lifecycle state of a workspace artifact.
///
/// Maps to the `artifact_state` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "artifact_state", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactState {
    /// Artifact is a draft, not yet finalized.
    Draft,
    /// Artifact has been committed/finalized.
    Committed,
    /// Artifact has been archived.
    Archived,
}

/// Relationship type between two artifacts.
///
/// Maps to the `artifact_relation` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "artifact_relation", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactRelation {
    /// Target artifact is derived from source.
    DerivedFrom,
    /// Target artifact supersedes source.
    Supersedes,
    /// Target artifact references source.
    References,
}

/// Kind of workspace artifact.
///
/// Maps to the `artifact_kind` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "artifact_kind", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    /// A code file or snippet.
    Code,
    /// A document (markdown, text, etc.).
    Document,
    /// A configuration file.
    Config,
    /// A generated output (build artifact, report, etc.).
    Generated,
}
```

**Step 2: Add tests for new enums**

Add to the existing `tests` module in `enums.rs`:

```rust
#[test]
fn job_status_serde_roundtrip() {
    for variant in [JobStatus::Active, JobStatus::Paused, JobStatus::Cancelled] {
        let json = serde_json::to_string(&variant).unwrap();
        let deserialized: JobStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(variant, deserialized);
    }
}

#[test]
fn artifact_state_serde_roundtrip() {
    for variant in [
        ArtifactState::Draft,
        ArtifactState::Committed,
        ArtifactState::Archived,
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        let deserialized: ArtifactState = serde_json::from_str(&json).unwrap();
        assert_eq!(variant, deserialized);
    }
}

#[test]
fn artifact_relation_serde_roundtrip() {
    for variant in [
        ArtifactRelation::DerivedFrom,
        ArtifactRelation::Supersedes,
        ArtifactRelation::References,
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        let deserialized: ArtifactRelation = serde_json::from_str(&json).unwrap();
        assert_eq!(variant, deserialized);
    }
}

#[test]
fn artifact_kind_serde_roundtrip() {
    for variant in [
        ArtifactKind::Code,
        ArtifactKind::Document,
        ArtifactKind::Config,
        ArtifactKind::Generated,
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        let deserialized: ArtifactKind = serde_json::from_str(&json).unwrap();
        assert_eq!(variant, deserialized);
    }
}
```

**Step 3: Update mod.rs re-exports**

In `types/mod.rs`, update the `enums` use statement:

```rust
pub use enums::{
    ArtifactKind, ArtifactRelation, ArtifactState, JobStatus, MessageRole, ScopeKind,
    UserStatus,
};
```

**Step 4: Run tests**

Run: `cd backend && cargo test -p sober-core -q`
Expected: PASS

**Step 5: Run clippy**

Run: `cd backend && cargo clippy -p sober-core -q -- -D warnings`
Expected: No warnings

**Step 6: Commit**

```
feat(core): add JobStatus, ArtifactState, ArtifactRelation, ArtifactKind enums
```

---

### Task 3: Add domain entity types to sober-core

**Files:**
- Create: `backend/crates/sober-core/src/types/domain.rs`
- Modify: `backend/crates/sober-core/src/types/mod.rs`

**Step 1: Create domain.rs with all entity structs**

```rust
//! Domain entity types used across all crates.
//!
//! These are the canonical representations of database entities. They are
//! returned by repo trait methods and consumed by business logic. Row types
//! (`FromRow` structs) are private to `sober-db` and convert into these.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::{
    ArtifactKind, ArtifactState, JobStatus, MessageRole, UserStatus,
};
use super::ids::{
    ArtifactId, AuditLogId, ConversationId, JobId, McpServerId, MessageId,
    RoleId, ScopeId, SessionId, UserId, WorkspaceId, WorkspaceRepoId, WorktreeId,
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
    /// When the job should next run.
    pub next_run_at: Option<DateTime<Utc>>,
    /// When the job last ran.
    pub last_run_at: Option<DateTime<Utc>>,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
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
    /// Root filesystem path.
    pub root_path: String,
    /// Whether the workspace is archived.
    pub archived: bool,
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
    /// Whether the worktree has been marked stale.
    pub stale: bool,
    /// When the worktree was created.
    pub created_at: DateTime<Utc>,
}

/// A workspace artifact (code, document, config, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// Unique identifier.
    pub id: ArtifactId,
    /// The workspace this artifact belongs to.
    pub workspace_id: WorkspaceId,
    /// Display name.
    pub name: String,
    /// Artifact kind.
    pub kind: ArtifactKind,
    /// Lifecycle state.
    pub state: ArtifactState,
    /// Filesystem path (relative to workspace root).
    pub path: String,
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
            tool_calls: Some(serde_json::json!([{"name": "web_search", "input": {"query": "rust"}}])),
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
            next_run_at: Some(Utc::now()),
            last_run_at: None,
            created_at: Utc::now(),
        };
        let json = serde_json::to_value(&job).unwrap();
        assert_eq!(json["status"], "active");
        assert_eq!(json["name"], "memory_prune");
    }
}
```

**Step 2: Update mod.rs**

Add `pub mod domain;` and re-exports:

```rust
pub mod domain;
```

And add re-exports:

```rust
pub use domain::{
    Artifact, AuditLogEntry, Conversation, Job, McpServerConfig, Message, Role, Session,
    User, UserRole, Workspace, WorkspaceRepoEntry, Worktree,
};
```

**Step 3: Run tests**

Run: `cd backend && cargo test -p sober-core -q`
Expected: PASS

**Step 4: Run clippy**

Run: `cd backend && cargo clippy -p sober-core -q -- -D warnings`
Expected: No warnings

**Step 5: Commit**

```
feat(core): add domain entity types for all database tables
```

---

### Task 4: Add input types to sober-core

**Files:**
- Create: `backend/crates/sober-core/src/types/input.rs`
- Modify: `backend/crates/sober-core/src/types/mod.rs`

**Step 1: Create input.rs with all input/filter structs**

```rust
//! Input types for repository create/update operations.
//!
//! These are passed to repo trait methods. They contain only the fields
//! the caller provides — IDs, timestamps, and defaults are set by the
//! repo implementation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::{ArtifactKind, ArtifactRelation, ArtifactState, JobStatus};
use super::ids::{JobId, McpServerId, UserId, WorkspaceId};

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
    pub conversation_id: super::ids::ConversationId,
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
```

**Step 2: Update mod.rs**

Add module and re-exports:

```rust
pub mod input;
```

```rust
pub use input::{
    ArtifactFilter, CreateArtifact, CreateAuditLog, CreateJob, CreateMcpServer,
    CreateMessage, CreateSession, CreateUser, RegisterRepo, UpdateMcpServer,
};
```

**Step 3: Run tests and clippy**

Run: `cd backend && cargo test -p sober-core -q && cargo clippy -p sober-core -q -- -D warnings`
Expected: PASS, no warnings

**Step 4: Commit**

```
feat(core): add input types for repo operations
```

---

### Task 5: Add repo trait definitions to sober-core

**Files:**
- Create: `backend/crates/sober-core/src/types/repo.rs`
- Modify: `backend/crates/sober-core/src/types/mod.rs`

**Step 1: Create repo.rs with all trait definitions**

Uses Rust 2024 RPITIT (no `async_trait` crate needed). Each method returns
`impl Future<Output = Result<T, AppError>> + Send`.

```rust
//! Repository trait definitions for database access.
//!
//! Traits are defined here in `sober-core` so library crates can depend on
//! them without importing `sqlx`. Concrete PostgreSQL implementations live
//! in `sober-db`.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::domain::*;
use super::enums::{ArtifactState, UserStatus};
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
    fn delete(
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
    fn delete(
        &self,
        id: WorkspaceRepoId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;
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
    fn get_by_id(
        &self,
        id: ArtifactId,
    ) -> impl Future<Output = Result<Artifact, AppError>> + Send;

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
        relation: super::enums::ArtifactRelation,
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
```

**Step 2: Update mod.rs**

Add module and re-exports:

```rust
pub mod repo;
```

```rust
pub use repo::{
    ArtifactRepo, AuditLogRepo, ConversationRepo, JobRepo, McpServerRepo, MessageRepo,
    SessionRepo, UserRepo, WorkspaceRepo, WorkspaceRepoRepo, WorktreeRepo,
};
```

**Step 3: Build to verify traits compile**

Run: `cd backend && cargo build -p sober-core -q`
Expected: Compiles successfully. Note: `use std::future::Future` is in the 2024 edition prelude, so no explicit import needed.

**Step 4: Run clippy**

Run: `cd backend && cargo clippy -p sober-core -q -- -D warnings`
Expected: No warnings

**Step 5: Commit**

```
feat(core): add repository trait definitions for all entities
```

---

## Phase 2: SQL Migrations

### Task 6: Create migrations directory and initial migrations

**Files:**
- Create: `backend/migrations/20260306000001_create_types.sql`
- Create: `backend/migrations/20260306000002_create_roles.sql`
- Create: `backend/migrations/20260306000003_create_users.sql`
- Create: `backend/migrations/20260306000004_create_user_roles.sql`
- Create: `backend/migrations/20260306000005_create_sessions.sql`
- Create: `backend/migrations/20260306000006_create_conversations.sql`
- Create: `backend/migrations/20260306000007_create_messages.sql`
- Create: `backend/migrations/20260306000008_create_mcp_servers.sql`
- Create: `backend/migrations/20260306000009_create_audit_log.sql`

**Step 1: Create each migration file**

`20260306000001_create_types.sql`:
```sql
CREATE TYPE user_status AS ENUM ('pending', 'active', 'disabled');
CREATE TYPE scope_kind AS ENUM ('system', 'user', 'group', 'session');
CREATE TYPE message_role AS ENUM ('user', 'assistant', 'system', 'tool');
CREATE TYPE job_status AS ENUM ('active', 'paused', 'cancelled');
CREATE TYPE artifact_state AS ENUM ('draft', 'committed', 'archived');
CREATE TYPE artifact_relation AS ENUM ('derived_from', 'supersedes', 'references');
CREATE TYPE artifact_kind AS ENUM ('code', 'document', 'config', 'generated');
```

`20260306000002_create_roles.sql`:
```sql
CREATE TABLE roles (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO roles (id, name, description) VALUES
    ('01960000-0000-7000-8000-000000000001', 'user',  'Default user role'),
    ('01960000-0000-7000-8000-000000000002', 'admin', 'Administrator role');
```

`20260306000003_create_users.sql`:
```sql
CREATE TABLE users (
    id            UUID PRIMARY KEY,
    email         TEXT NOT NULL UNIQUE,
    username      TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    status        user_status NOT NULL DEFAULT 'pending',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_users_email ON users (email);
CREATE INDEX idx_users_username ON users (username);
CREATE INDEX idx_users_status ON users (status);
```

`20260306000004_create_user_roles.sql`:
```sql
CREATE TABLE user_roles (
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role_id    UUID NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    scope_id   UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    granted_by UUID REFERENCES users (id) ON DELETE SET NULL,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (user_id, role_id, scope_id)
);

CREATE INDEX idx_user_roles_user_id ON user_roles (user_id);
CREATE INDEX idx_user_roles_role_id ON user_roles (role_id);
CREATE INDEX idx_user_roles_scope_id ON user_roles (scope_id)
    WHERE scope_id != '00000000-0000-0000-0000-000000000000';
```

`20260306000005_create_sessions.sql`:
```sql
CREATE TABLE sessions (
    id         UUID PRIMARY KEY,
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_sessions_user_id ON sessions (user_id);
CREATE INDEX idx_sessions_token_hash ON sessions (token_hash);
CREATE INDEX idx_sessions_expires_at ON sessions (expires_at);
```

`20260306000006_create_conversations.sql`:
```sql
CREATE TABLE conversations (
    id         UUID PRIMARY KEY,
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    title      TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conversations_user_id ON conversations (user_id);
CREATE INDEX idx_conversations_updated_at ON conversations (updated_at DESC);
```

`20260306000007_create_messages.sql`:
```sql
CREATE TABLE messages (
    id              UUID PRIMARY KEY,
    conversation_id UUID NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    role            message_role NOT NULL,
    content         TEXT NOT NULL,
    tool_calls      JSONB,
    tool_result     JSONB,
    token_count     INTEGER,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_messages_conversation_id ON messages (conversation_id);
CREATE INDEX idx_messages_created_at ON messages (created_at);
CREATE INDEX idx_messages_role ON messages (role);
```

`20260306000008_create_mcp_servers.sql`:
```sql
CREATE TABLE mcp_servers (
    id         UUID PRIMARY KEY,
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    command    TEXT NOT NULL,
    args       JSONB NOT NULL DEFAULT '[]',
    env        JSONB NOT NULL DEFAULT '{}',
    enabled    BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT uq_mcp_servers_user_name UNIQUE (user_id, name)
);

CREATE INDEX idx_mcp_servers_user_id ON mcp_servers (user_id);
CREATE INDEX idx_mcp_servers_enabled ON mcp_servers (user_id, enabled) WHERE enabled = true;
```

`20260306000009_create_audit_log.sql`:
```sql
CREATE TABLE audit_log (
    id          UUID PRIMARY KEY,
    actor_id    UUID REFERENCES users (id) ON DELETE SET NULL,
    action      TEXT NOT NULL,
    target_type TEXT,
    target_id   UUID,
    details     JSONB,
    ip_address  INET,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_audit_log_actor_id ON audit_log (actor_id) WHERE actor_id IS NOT NULL;
CREATE INDEX idx_audit_log_action ON audit_log (action);
CREATE INDEX idx_audit_log_target ON audit_log (target_type, target_id)
    WHERE target_type IS NOT NULL;
CREATE INDEX idx_audit_log_created_at ON audit_log (created_at DESC);
```

**Step 2: Commit**

```
feat(db): add SQL migrations for all v1 tables
```

---

### Task 7: Add job and workspace table migrations

**Files:**
- Create: `backend/migrations/20260306000010_create_jobs.sql`
- Create: `backend/migrations/20260306000011_create_workspaces.sql`
- Create: `backend/migrations/20260306000012_create_workspace_repos.sql`
- Create: `backend/migrations/20260306000013_create_worktrees.sql`
- Create: `backend/migrations/20260306000014_create_artifacts.sql`
- Create: `backend/migrations/20260306000015_create_artifact_relations.sql`

**Step 1: Create each migration file**

`20260306000010_create_jobs.sql`:
```sql
CREATE TABLE jobs (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    schedule    TEXT NOT NULL,
    status      job_status NOT NULL DEFAULT 'active',
    payload     JSONB NOT NULL DEFAULT '{}',
    next_run_at TIMESTAMPTZ,
    last_run_at TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_jobs_status ON jobs (status);
CREATE INDEX idx_jobs_next_run ON jobs (next_run_at) WHERE status = 'active';
```

`20260306000011_create_workspaces.sql`:
```sql
CREATE TABLE workspaces (
    id         UUID PRIMARY KEY,
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    root_path  TEXT NOT NULL,
    archived   BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_workspaces_user_id ON workspaces (user_id);
CREATE INDEX idx_workspaces_archived ON workspaces (user_id, archived)
    WHERE archived = false;
```

`20260306000012_create_workspace_repos.sql`:
```sql
CREATE TABLE workspace_repos (
    id              UUID PRIMARY KEY,
    workspace_id    UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    path            TEXT NOT NULL,
    default_branch  TEXT NOT NULL DEFAULT 'main',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT uq_workspace_repos_path UNIQUE (workspace_id, path)
);

CREATE INDEX idx_workspace_repos_workspace_id ON workspace_repos (workspace_id);
```

`20260306000013_create_worktrees.sql`:
```sql
CREATE TABLE worktrees (
    id         UUID PRIMARY KEY,
    repo_id    UUID NOT NULL REFERENCES workspace_repos (id) ON DELETE CASCADE,
    branch     TEXT NOT NULL,
    path       TEXT NOT NULL,
    stale      BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_worktrees_repo_id ON worktrees (repo_id);
CREATE INDEX idx_worktrees_stale ON worktrees (stale, created_at)
    WHERE stale = false;
```

`20260306000014_create_artifacts.sql`:
```sql
CREATE TABLE artifacts (
    id           UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    kind         artifact_kind NOT NULL,
    state        artifact_state NOT NULL DEFAULT 'draft',
    path         TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_artifacts_workspace_id ON artifacts (workspace_id);
CREATE INDEX idx_artifacts_kind ON artifacts (workspace_id, kind);
CREATE INDEX idx_artifacts_state ON artifacts (workspace_id, state);
```

`20260306000015_create_artifact_relations.sql`:
```sql
CREATE TABLE artifact_relations (
    source_id UUID NOT NULL REFERENCES artifacts (id) ON DELETE CASCADE,
    target_id UUID NOT NULL REFERENCES artifacts (id) ON DELETE CASCADE,
    relation  artifact_relation NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (source_id, target_id, relation)
);

CREATE INDEX idx_artifact_relations_target ON artifact_relations (target_id);
```

**Step 2: Commit**

```
feat(db): add migrations for jobs, workspaces, worktrees, artifacts
```

---

## Phase 3: sober-db Crate

### Task 8: Create sober-db crate with pool creation

**Files:**
- Create: `backend/crates/sober-db/Cargo.toml`
- Create: `backend/crates/sober-db/src/lib.rs`
- Create: `backend/crates/sober-db/src/pool.rs`

**Step 1: Create Cargo.toml**

```toml
[package]
name = "sober-db"
version = "0.1.0"
edition.workspace = true

[dependencies]
sober-core = { path = "../sober-core", features = ["postgres"] }
sqlx = { workspace = true }
tracing = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
serde_json = { workspace = true }

[dev-dependencies]
tokio = { workspace = true }
```

**Step 2: Create pool.rs**

```rust
//! PostgreSQL connection pool creation.

use std::time::Duration;

use sober_core::config::DatabaseConfig;
use sober_core::AppError;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Creates a PostgreSQL connection pool with the given configuration.
///
/// Uses the configured `max_connections` and a 5-second acquire timeout.
pub async fn create_pool(config: &DatabaseConfig) -> Result<PgPool, AppError> {
    PgPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.url)
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))
}

/// Creates a minimal connection pool (1 connection) for CLI tools.
///
/// CLI tools perform sequential operations and don't need concurrency.
pub async fn create_cli_pool(database_url: &str) -> Result<PgPool, AppError> {
    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))
}
```

**Step 3: Create lib.rs**

```rust
//! Centralized PostgreSQL access layer for the Sober system.
//!
//! This crate is the only place in the workspace that depends on `sqlx`.
//! It provides:
//!
//! - [`create_pool`] / [`create_cli_pool`] — connection pool creation
//! - `Pg*Repo` structs — concrete implementations of repo traits from `sober-core`
//!
//! Library crates depend only on `sober-core` repo traits. Binary crates
//! construct `Pg*Repo` instances at startup and pass them as `Arc<dyn Repo>`.

mod pool;

pub use pool::{create_cli_pool, create_pool};

// Re-export PgPool for binary crates that need pool lifecycle management.
pub use sqlx::PgPool;
```

**Step 4: Build**

Run: `cd backend && cargo build -p sober-db -q`
Expected: Compiles successfully.

**Step 5: Commit**

```
feat(db): create sober-db crate with pool creation
```

---

### Task 9: Implement PgUserRepo

**Files:**
- Create: `backend/crates/sober-db/src/rows.rs`
- Create: `backend/crates/sober-db/src/repos/mod.rs`
- Create: `backend/crates/sober-db/src/repos/user.rs`
- Modify: `backend/crates/sober-db/src/lib.rs`

**Step 1: Create rows.rs with UserRow**

```rust
//! Private row types that map directly to database columns.
//!
//! These are never exposed publicly. Each row type has a `From` implementation
//! that converts it to the corresponding domain type from `sober-core`.

use chrono::{DateTime, Utc};
use sober_core::types::enums::UserStatus;
use sober_core::types::ids::UserId;
use sober_core::User;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct UserRow {
    pub id: Uuid,
    pub email: String,
    pub username: String,
    pub password_hash: String,
    pub status: UserStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<UserRow> for User {
    fn from(row: UserRow) -> Self {
        User {
            id: UserId::from_uuid(row.id),
            email: row.email,
            username: row.username,
            status: row.status,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
```

**Step 2: Create repos/user.rs**

```rust
//! PostgreSQL implementation of [`UserRepo`].

use sober_core::types::enums::UserStatus;
use sober_core::types::ids::UserId;
use sober_core::types::input::CreateUser;
use sober_core::types::repo::UserRepo;
use sober_core::{AppError, User};
use sqlx::PgPool;

use crate::rows::UserRow;

/// PostgreSQL-backed user repository.
pub struct PgUserRepo {
    pool: PgPool,
}

impl PgUserRepo {
    /// Creates a new `PgUserRepo` backed by the given pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl UserRepo for PgUserRepo {
    async fn get_by_id(&self, id: UserId) -> Result<User, AppError> {
        let row = sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(Box::new(e)))?
            .ok_or_else(|| AppError::NotFound("user".into()))?;
        Ok(row.into())
    }

    async fn get_by_email(&self, email: &str) -> Result<User, AppError> {
        let row = sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE email = $1")
            .bind(email)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(Box::new(e)))?
            .ok_or_else(|| AppError::NotFound("user".into()))?;
        Ok(row.into())
    }

    async fn get_by_username(&self, username: &str) -> Result<User, AppError> {
        let row = sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE username = $1")
            .bind(username)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AppError::Internal(Box::new(e)))?
            .ok_or_else(|| AppError::NotFound("user".into()))?;
        Ok(row.into())
    }

    async fn create(&self, input: CreateUser) -> Result<User, AppError> {
        let id = UserId::new();
        let row = sqlx::query_as::<_, UserRow>(
            "INSERT INTO users (id, email, username, password_hash) \
             VALUES ($1, $2, $3, $4) RETURNING *",
        )
        .bind(id)
        .bind(&input.email)
        .bind(&input.username)
        .bind(&input.password_hash)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))?;
        Ok(row.into())
    }

    async fn create_with_role(&self, input: CreateUser, role: &str) -> Result<User, AppError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(Box::new(e)))?;

        let id = UserId::new();
        let row = sqlx::query_as::<_, UserRow>(
            "INSERT INTO users (id, email, username, password_hash) \
             VALUES ($1, $2, $3, $4) RETURNING *",
        )
        .bind(id)
        .bind(&input.email)
        .bind(&input.username)
        .bind(&input.password_hash)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))?;

        sqlx::query(
            "INSERT INTO user_roles (user_id, role_id) \
             SELECT $1, id FROM roles WHERE name = $2",
        )
        .bind(row.id)
        .bind(role)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(Box::new(e)))?;
        Ok(row.into())
    }

    async fn update_status(&self, id: UserId, status: UserStatus) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE users SET status = $1, updated_at = now() WHERE id = $2",
        )
        .bind(status)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("user".into()));
        }
        Ok(())
    }

    async fn get_password_hash(&self, id: UserId) -> Result<String, AppError> {
        let row: (String,) =
            sqlx::query_as("SELECT password_hash FROM users WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AppError::Internal(Box::new(e)))?
                .ok_or_else(|| AppError::NotFound("user".into()))?;
        Ok(row.0)
    }
}
```

**Step 3: Create repos/mod.rs**

```rust
//! Concrete PostgreSQL repository implementations.

mod user;

pub use user::PgUserRepo;
```

**Step 4: Update lib.rs**

Add modules and re-exports:

```rust
mod pool;
mod repos;
mod rows;

pub use pool::{create_cli_pool, create_pool};
pub use repos::*;
pub use sqlx::PgPool;
```

**Step 5: Build**

Run: `cd backend && cargo build -p sober-db -q`
Expected: Compiles successfully.

**Step 6: Commit**

```
feat(db): implement PgUserRepo with row types
```

---

### Task 10: Implement PgSessionRepo and PgConversationRepo

**Files:**
- Modify: `backend/crates/sober-db/src/rows.rs` (add SessionRow, ConversationRow)
- Create: `backend/crates/sober-db/src/repos/session.rs`
- Create: `backend/crates/sober-db/src/repos/conversation.rs`
- Modify: `backend/crates/sober-db/src/repos/mod.rs`

**Step 1: Add row types to rows.rs**

```rust
use sober_core::types::ids::{ConversationId, SessionId};
use sober_core::{Conversation, Session};

#[derive(sqlx::FromRow)]
pub(crate) struct SessionRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl From<SessionRow> for Session {
    fn from(row: SessionRow) -> Self {
        Session {
            id: SessionId::from_uuid(row.id),
            user_id: UserId::from_uuid(row.user_id),
            token_hash: row.token_hash,
            expires_at: row.expires_at,
            created_at: row.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct ConversationRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<ConversationRow> for Conversation {
    fn from(row: ConversationRow) -> Self {
        Conversation {
            id: ConversationId::from_uuid(row.id),
            user_id: UserId::from_uuid(row.user_id),
            title: row.title,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
```

**Step 2: Create repos/session.rs**

```rust
//! PostgreSQL implementation of [`SessionRepo`].

use sober_core::types::ids::SessionId;
use sober_core::types::input::CreateSession;
use sober_core::types::repo::SessionRepo;
use sober_core::{AppError, Session};
use sqlx::PgPool;

use crate::rows::SessionRow;

/// PostgreSQL-backed session repository.
pub struct PgSessionRepo {
    pool: PgPool,
}

impl PgSessionRepo {
    /// Creates a new `PgSessionRepo`.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl SessionRepo for PgSessionRepo {
    async fn get_by_token_hash(&self, token_hash: &str) -> Result<Option<Session>, AppError> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT * FROM sessions WHERE token_hash = $1 AND expires_at > now()",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))?;
        Ok(row.map(Into::into))
    }

    async fn create(&self, input: CreateSession) -> Result<Session, AppError> {
        let id = SessionId::new();
        let row = sqlx::query_as::<_, SessionRow>(
            "INSERT INTO sessions (id, user_id, token_hash, expires_at) \
             VALUES ($1, $2, $3, $4) RETURNING *",
        )
        .bind(id)
        .bind(input.user_id)
        .bind(&input.token_hash)
        .bind(input.expires_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))?;
        Ok(row.into())
    }

    async fn delete_by_token_hash(&self, token_hash: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(token_hash)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(Box::new(e)))?;
        Ok(())
    }

    async fn cleanup_expired(&self) -> Result<u64, AppError> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= now()")
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(Box::new(e)))?;
        Ok(result.rows_affected())
    }
}
```

**Step 3: Create repos/conversation.rs**

```rust
//! PostgreSQL implementation of [`ConversationRepo`].

use sober_core::types::ids::{ConversationId, UserId};
use sober_core::types::repo::ConversationRepo;
use sober_core::{AppError, Conversation};
use sqlx::PgPool;

use crate::rows::ConversationRow;

/// PostgreSQL-backed conversation repository.
pub struct PgConversationRepo {
    pool: PgPool,
}

impl PgConversationRepo {
    /// Creates a new `PgConversationRepo`.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl ConversationRepo for PgConversationRepo {
    async fn create(
        &self,
        user_id: UserId,
        title: Option<&str>,
    ) -> Result<Conversation, AppError> {
        let id = ConversationId::new();
        let row = sqlx::query_as::<_, ConversationRow>(
            "INSERT INTO conversations (id, user_id, title) VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(id)
        .bind(user_id)
        .bind(title)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))?;
        Ok(row.into())
    }

    async fn get_by_id(&self, id: ConversationId) -> Result<Conversation, AppError> {
        let row = sqlx::query_as::<_, ConversationRow>(
            "SELECT * FROM conversations WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))?
        .ok_or_else(|| AppError::NotFound("conversation".into()))?;
        Ok(row.into())
    }

    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<Conversation>, AppError> {
        let rows = sqlx::query_as::<_, ConversationRow>(
            "SELECT * FROM conversations WHERE user_id = $1 ORDER BY updated_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn update_title(&self, id: ConversationId, title: &str) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE conversations SET title = $1, updated_at = now() WHERE id = $2",
        )
        .bind(title)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(Box::new(e)))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("conversation".into()));
        }
        Ok(())
    }

    async fn delete(&self, id: ConversationId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM conversations WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(Box::new(e)))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("conversation".into()));
        }
        Ok(())
    }
}
```

**Step 4: Update repos/mod.rs**

```rust
mod conversation;
mod session;
mod user;

pub use conversation::PgConversationRepo;
pub use session::PgSessionRepo;
pub use user::PgUserRepo;
```

**Step 5: Build**

Run: `cd backend && cargo build -p sober-db -q`
Expected: Compiles successfully.

**Step 6: Commit**

```
feat(db): implement PgSessionRepo and PgConversationRepo
```

---

### Task 11: Implement PgMessageRepo and PgMcpServerRepo

**Files:**
- Modify: `backend/crates/sober-db/src/rows.rs` (add MessageRow, McpServerRow)
- Create: `backend/crates/sober-db/src/repos/message.rs`
- Create: `backend/crates/sober-db/src/repos/mcp_server.rs`
- Modify: `backend/crates/sober-db/src/repos/mod.rs`

Follow the same pattern as Task 10. Add `MessageRow` and `McpServerRow` to `rows.rs`,
implement `MessageRepo` for `PgMessageRepo` and `McpServerRepo` for `PgMcpServerRepo`.

Key implementation notes:
- `PgMessageRepo::list_by_conversation` — ORDER BY `created_at ASC` with `LIMIT $2`
- `PgMcpServerRepo::update` — build a dynamic UPDATE with only non-None fields from `UpdateMcpServer`, always set `updated_at = now()`. Use a RETURNING clause.

**Commit:**

```
feat(db): implement PgMessageRepo and PgMcpServerRepo
```

---

### Task 12: Implement PgJobRepo and PgAuditLogRepo

**Files:**
- Modify: `backend/crates/sober-db/src/rows.rs` (add JobRow, AuditLogRow)
- Create: `backend/crates/sober-db/src/repos/job.rs`
- Create: `backend/crates/sober-db/src/repos/audit_log.rs`
- Modify: `backend/crates/sober-db/src/repos/mod.rs`

Follow the same pattern. Key notes:
- `PgJobRepo::list_active` — `WHERE status = 'active'` cast to the enum type
- `PgJobRepo::cancel` — `UPDATE jobs SET status = 'cancelled' WHERE id = $1`
- `PgAuditLogRepo` — append-only, no update or delete methods
- `audit_log.ip_address` is `INET` in PostgreSQL. Store as `String` in domain type, cast with `$1::inet` in SQL.

**Commit:**

```
feat(db): implement PgJobRepo and PgAuditLogRepo
```

---

### Task 13: Implement PgWorkspaceRepo, PgWorkspaceRepoRepo, PgWorktreeRepo, PgArtifactRepo

**Files:**
- Modify: `backend/crates/sober-db/src/rows.rs` (add remaining row types)
- Create: `backend/crates/sober-db/src/repos/workspace.rs`
- Create: `backend/crates/sober-db/src/repos/workspace_repo.rs`
- Create: `backend/crates/sober-db/src/repos/worktree.rs`
- Create: `backend/crates/sober-db/src/repos/artifact.rs`
- Modify: `backend/crates/sober-db/src/repos/mod.rs`

Follow the same pattern. Key notes:
- `PgWorkspaceRepo::archive` — `UPDATE workspaces SET archived = true, updated_at = now()`
- `PgWorkspaceRepo::restore` — `UPDATE workspaces SET archived = false, updated_at = now()`
- `PgArtifactRepo::list_by_workspace` — build WHERE clause dynamically based on `ArtifactFilter` fields
- `PgArtifactRepo::add_relation` — INSERT into `artifact_relations` table

**Commit:**

```
feat(db): implement workspace, worktree, and artifact repos
```

---

## Phase 4: Integration Tests

### Task 14: Set up integration test harness and test all repos

**Files:**
- Create: `backend/crates/sober-db/tests/repos.rs`

**Prerequisites:**
- Docker must be running (`docker compose up -d` for PostgreSQL)
- `DATABASE_URL` must be set (for `sqlx::test`)

**Step 1: Create test file using `#[sqlx::test]`**

`sqlx::test` automatically creates a temporary test database, runs migrations, and
rolls back after each test. Each test gets an isolated `PgPool`.

```rust
use sober_core::types::enums::*;
use sober_core::types::input::*;
use sober_core::types::repo::*;
use sober_db::*;
use sqlx::PgPool;

// --- User tests ---

#[sqlx::test(migrations = "../migrations")]
async fn create_user(pool: PgPool) {
    let repo = PgUserRepo::new(pool);
    let user = repo
        .create(CreateUser {
            email: "test@example.com".into(),
            username: "testuser".into(),
            password_hash: "$argon2id$fake_hash".into(),
        })
        .await
        .unwrap();
    assert_eq!(user.email, "test@example.com");
    assert_eq!(user.status, UserStatus::Pending);
}

#[sqlx::test(migrations = "../migrations")]
async fn get_user_by_email(pool: PgPool) {
    let repo = PgUserRepo::new(pool);
    repo.create(CreateUser {
        email: "find@example.com".into(),
        username: "findme".into(),
        password_hash: "hash".into(),
    })
    .await
    .unwrap();

    let found = repo.get_by_email("find@example.com").await.unwrap();
    assert_eq!(found.username, "findme");
}

#[sqlx::test(migrations = "../migrations")]
async fn create_user_with_role(pool: PgPool) {
    let repo = PgUserRepo::new(pool);
    let user = repo
        .create_with_role(
            CreateUser {
                email: "admin@example.com".into(),
                username: "admin".into(),
                password_hash: "hash".into(),
            },
            "admin",
        )
        .await
        .unwrap();
    assert_eq!(user.email, "admin@example.com");
}

#[sqlx::test(migrations = "../migrations")]
async fn update_user_status(pool: PgPool) {
    let repo = PgUserRepo::new(pool);
    let user = repo
        .create(CreateUser {
            email: "status@example.com".into(),
            username: "statususer".into(),
            password_hash: "hash".into(),
        })
        .await
        .unwrap();

    repo.update_status(user.id, UserStatus::Active).await.unwrap();
    let updated = repo.get_by_id(user.id).await.unwrap();
    assert_eq!(updated.status, UserStatus::Active);
}

#[sqlx::test(migrations = "../migrations")]
async fn get_password_hash(pool: PgPool) {
    let repo = PgUserRepo::new(pool);
    let user = repo
        .create(CreateUser {
            email: "pw@example.com".into(),
            username: "pwuser".into(),
            password_hash: "$argon2id$real_hash".into(),
        })
        .await
        .unwrap();

    let hash = repo.get_password_hash(user.id).await.unwrap();
    assert_eq!(hash, "$argon2id$real_hash");
}

// --- Session tests ---

#[sqlx::test(migrations = "../migrations")]
async fn session_lifecycle(pool: PgPool) {
    let user_repo = PgUserRepo::new(pool.clone());
    let session_repo = PgSessionRepo::new(pool);

    let user = user_repo
        .create(CreateUser {
            email: "sess@example.com".into(),
            username: "sessuser".into(),
            password_hash: "hash".into(),
        })
        .await
        .unwrap();

    let session = session_repo
        .create(CreateSession {
            user_id: user.id,
            token_hash: "abc123hash".into(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        })
        .await
        .unwrap();
    assert_eq!(session.user_id, user.id);

    let found = session_repo.get_by_token_hash("abc123hash").await.unwrap();
    assert!(found.is_some());

    session_repo.delete_by_token_hash("abc123hash").await.unwrap();
    let gone = session_repo.get_by_token_hash("abc123hash").await.unwrap();
    assert!(gone.is_none());
}

// --- Conversation + Message tests ---

#[sqlx::test(migrations = "../migrations")]
async fn conversation_and_messages(pool: PgPool) {
    let user_repo = PgUserRepo::new(pool.clone());
    let conv_repo = PgConversationRepo::new(pool.clone());
    let msg_repo = PgMessageRepo::new(pool);

    let user = user_repo
        .create(CreateUser {
            email: "conv@example.com".into(),
            username: "convuser".into(),
            password_hash: "hash".into(),
        })
        .await
        .unwrap();

    let conv = conv_repo
        .create(user.id, Some("Test Chat"))
        .await
        .unwrap();
    assert_eq!(conv.title.as_deref(), Some("Test Chat"));

    let msg = msg_repo
        .create(CreateMessage {
            conversation_id: conv.id,
            role: MessageRole::User,
            content: "Hello!".into(),
            tool_calls: None,
            tool_result: None,
            token_count: Some(5),
        })
        .await
        .unwrap();
    assert_eq!(msg.content, "Hello!");

    let messages = msg_repo.list_by_conversation(conv.id, 10).await.unwrap();
    assert_eq!(messages.len(), 1);

    conv_repo.update_title(conv.id, "Renamed").await.unwrap();
    let updated = conv_repo.get_by_id(conv.id).await.unwrap();
    assert_eq!(updated.title.as_deref(), Some("Renamed"));

    conv_repo.delete(conv.id).await.unwrap();
    assert!(conv_repo.get_by_id(conv.id).await.is_err());
}

// Add similar tests for remaining repos (MCP, Job, Workspace, Worktree, Artifact, AuditLog)
// following the same pattern: create dependencies first, then test CRUD operations.
```

**Step 2: Run integration tests**

Run: `cd backend && docker compose up -d && cargo test -p sober-db -q`
Expected: All tests pass.

**Step 3: Run clippy on the whole crate**

Run: `cd backend && cargo clippy -p sober-db -q -- -D warnings`
Expected: No warnings.

**Step 4: Commit**

```
test(db): add integration tests for all Pg*Repo implementations
```

---

## Phase 5: Verification & Cleanup

### Task 15: Final verification and cross-crate build

**Step 1: Run full workspace build**

Run: `cd backend && cargo build -q`
Expected: Entire workspace compiles.

**Step 2: Run all tests**

Run: `cd backend && cargo test --workspace -q`
Expected: All tests pass.

**Step 3: Run workspace clippy**

Run: `cd backend && cargo clippy --workspace -q -- -D warnings`
Expected: No warnings.

**Step 4: Generate docs**

Run: `cd backend && cargo doc -p sober-core --no-deps -q && cargo doc -p sober-db --no-deps -q`
Expected: No warnings.

**Step 5: Prepare sqlx offline data**

Run: `cd backend && cargo sqlx prepare --workspace -q`
Expected: `.sqlx/` directory updated with query metadata for CI builds.

**Step 6: Move plan 003 to done, 005 to active**

```bash
git mv docs/plans/active/003-sober-core docs/plans/done/003-sober-core
git mv docs/plans/pending/005-sober-db docs/plans/active/005-sober-db
```

**Step 7: Commit**

```
docs: move 003 to done, 005 to active
```

---

## Acceptance Criteria

- [ ] All domain types compile and are importable via `use sober_core::*`
- [ ] All repo traits compile with RPITIT (no `async_trait` dependency)
- [ ] `sober-db` is the only crate with `sqlx` in its dependency tree (besides `sober-core`'s optional `postgres` feature for type derives)
- [ ] All 15 SQL migrations run cleanly against fresh PostgreSQL
- [ ] Integration tests cover CRUD operations for all repos
- [ ] `cargo clippy --workspace -q -- -D warnings` reports zero warnings
- [ ] `cargo doc -p sober-core --no-deps -q` and `cargo doc -p sober-db --no-deps -q` generate docs without warnings
- [ ] No `.unwrap()` in library code (only in tests)
- [ ] All public items have doc comments
