//! Private row types for sqlx `FromRow` deserialization.
//!
//! These structs map directly to database columns and convert to domain
//! types from `sober-core`. They are never exposed outside this crate.

use chrono::{DateTime, Utc};
use sober_core::types::{
    Artifact, AuditLogEntry, Conversation, Job, McpServerConfig, Message, Role, Session, User,
    UserRole, Workspace, WorkspaceRepoEntry, Worktree,
};
use sober_core::types::{
    ArtifactId, ArtifactKind, ArtifactState, AuditLogId, ConversationId, JobId, JobStatus,
    McpServerId, MessageId, MessageRole, RoleId, ScopeId, SessionId, UserId, UserStatus,
    WorkspaceId, WorkspaceRepoId, WorktreeId,
};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct UserRow {
    pub id: Uuid,
    pub email: String,
    pub username: String,
    #[allow(dead_code)] // Present in DB row but absent from domain type
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

#[derive(sqlx::FromRow)]
pub(crate) struct RoleRow {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
}

impl From<RoleRow> for Role {
    fn from(row: RoleRow) -> Self {
        Role {
            id: RoleId::from_uuid(row.id),
            name: row.name,
            description: row.description,
            created_at: row.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct UserRoleRow {
    pub user_id: Uuid,
    pub role_id: Uuid,
    pub scope_id: Uuid,
    pub granted_by: Option<Uuid>,
    pub granted_at: DateTime<Utc>,
}

impl From<UserRoleRow> for UserRole {
    fn from(row: UserRoleRow) -> Self {
        UserRole {
            user_id: UserId::from_uuid(row.user_id),
            role_id: RoleId::from_uuid(row.role_id),
            scope_id: ScopeId::from_uuid(row.scope_id),
            granted_by: row.granted_by.map(UserId::from_uuid),
            granted_at: row.granted_at,
        }
    }
}

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

#[derive(sqlx::FromRow)]
pub(crate) struct MessageRow {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub role: MessageRole,
    pub content: String,
    pub tool_calls: Option<serde_json::Value>,
    pub tool_result: Option<serde_json::Value>,
    pub token_count: Option<i32>,
    pub created_at: DateTime<Utc>,
}

impl From<MessageRow> for Message {
    fn from(row: MessageRow) -> Self {
        Message {
            id: MessageId::from_uuid(row.id),
            conversation_id: ConversationId::from_uuid(row.conversation_id),
            role: row.role,
            content: row.content,
            tool_calls: row.tool_calls,
            tool_result: row.tool_result,
            token_count: row.token_count,
            created_at: row.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct McpServerRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub command: String,
    pub args: serde_json::Value,
    pub env: serde_json::Value,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<McpServerRow> for McpServerConfig {
    fn from(row: McpServerRow) -> Self {
        McpServerConfig {
            id: McpServerId::from_uuid(row.id),
            user_id: UserId::from_uuid(row.user_id),
            name: row.name,
            command: row.command,
            args: row.args,
            env: row.env,
            enabled: row.enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct JobRow {
    pub id: Uuid,
    pub name: String,
    pub schedule: String,
    pub status: JobStatus,
    pub payload: serde_json::Value,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl From<JobRow> for Job {
    fn from(row: JobRow) -> Self {
        Job {
            id: JobId::from_uuid(row.id),
            name: row.name,
            schedule: row.schedule,
            status: row.status,
            payload: row.payload,
            next_run_at: row.next_run_at,
            last_run_at: row.last_run_at,
            created_at: row.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct WorkspaceRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub root_path: String,
    pub archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<WorkspaceRow> for Workspace {
    fn from(row: WorkspaceRow) -> Self {
        Workspace {
            id: WorkspaceId::from_uuid(row.id),
            user_id: UserId::from_uuid(row.user_id),
            name: row.name,
            root_path: row.root_path,
            archived: row.archived,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct WorkspaceRepoRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub path: String,
    pub default_branch: String,
    pub created_at: DateTime<Utc>,
}

impl From<WorkspaceRepoRow> for WorkspaceRepoEntry {
    fn from(row: WorkspaceRepoRow) -> Self {
        WorkspaceRepoEntry {
            id: WorkspaceRepoId::from_uuid(row.id),
            workspace_id: WorkspaceId::from_uuid(row.workspace_id),
            name: row.name,
            path: row.path,
            default_branch: row.default_branch,
            created_at: row.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct WorktreeRow {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub branch: String,
    pub path: String,
    pub stale: bool,
    pub created_at: DateTime<Utc>,
}

impl From<WorktreeRow> for Worktree {
    fn from(row: WorktreeRow) -> Self {
        Worktree {
            id: WorktreeId::from_uuid(row.id),
            repo_id: WorkspaceRepoId::from_uuid(row.repo_id),
            branch: row.branch,
            path: row.path,
            stale: row.stale,
            created_at: row.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct ArtifactRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub kind: ArtifactKind,
    pub state: ArtifactState,
    pub path: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<ArtifactRow> for Artifact {
    fn from(row: ArtifactRow) -> Self {
        Artifact {
            id: ArtifactId::from_uuid(row.id),
            workspace_id: WorkspaceId::from_uuid(row.workspace_id),
            name: row.name,
            kind: row.kind,
            state: row.state,
            path: row.path,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct AuditLogRow {
    pub id: Uuid,
    pub actor_id: Option<Uuid>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<Uuid>,
    pub details: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<AuditLogRow> for AuditLogEntry {
    fn from(row: AuditLogRow) -> Self {
        AuditLogEntry {
            id: AuditLogId::from_uuid(row.id),
            actor_id: row.actor_id.map(UserId::from_uuid),
            action: row.action,
            target_type: row.target_type,
            target_id: row.target_id,
            details: row.details,
            ip_address: row.ip_address,
            created_at: row.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn user_row_to_domain_excludes_password_hash() {
        let row = UserRow {
            id: Uuid::now_v7(),
            email: "test@example.com".into(),
            username: "testuser".into(),
            password_hash: "argon2id$secret".into(),
            status: UserStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let user: User = row.into();
        assert_eq!(user.email, "test@example.com");
        // User domain type has no password_hash field
    }

    #[test]
    fn job_row_to_domain() {
        let row = JobRow {
            id: Uuid::now_v7(),
            name: "test_job".into(),
            schedule: "0 * * * *".into(),
            status: JobStatus::Active,
            payload: serde_json::json!({}),
            next_run_at: None,
            last_run_at: None,
            created_at: Utc::now(),
        };
        let job: Job = row.into();
        assert_eq!(job.name, "test_job");
        assert_eq!(job.status, JobStatus::Active);
    }
}
