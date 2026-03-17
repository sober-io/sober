//! Private row types for sqlx `FromRow` deserialization.
//!
//! These structs map directly to database columns and convert to domain
//! types from `sober-core`. They are never exposed outside this crate.

use chrono::{DateTime, Utc};
use sober_core::types::{
    AgentMode, ArtifactId, ArtifactKind, ArtifactState, AuditLogId, ConversationId,
    ConversationKind, ConversationUserRole, EncryptionKeyId, JobId, JobRunId, JobStatus,
    McpServerId, MessageId, MessageRole, RoleId, ScopeId, SecretId, SessionId, TagId, UserId,
    UserStatus, WorkspaceId, WorkspaceRepoId, WorkspaceState, WorktreeId, WorktreeState,
};
use sober_core::types::{
    Artifact, AuditLogEntry, Conversation, ConversationUser, ConversationUserWithUsername, Job,
    JobRun, McpServerConfig, Message, Role, SecretMetadata, SecretRow, Session, StoredDek, Tag,
    User, UserRole, Workspace, WorkspaceRepoEntry, Worktree,
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
    pub workspace_id: Option<Uuid>,
    pub kind: ConversationKind,
    pub agent_mode: AgentMode,
    pub is_archived: bool,
    pub permission_mode: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<ConversationRow> for Conversation {
    fn from(row: ConversationRow) -> Self {
        use sober_core::PermissionMode;
        let permission_mode = match row.permission_mode.as_str() {
            "interactive" => PermissionMode::Interactive,
            "autonomous" => PermissionMode::Autonomous,
            _ => PermissionMode::PolicyBased,
        };
        Conversation {
            id: ConversationId::from_uuid(row.id),
            user_id: UserId::from_uuid(row.user_id),
            title: row.title,
            workspace_id: row.workspace_id.map(WorkspaceId::from_uuid),
            kind: row.kind,
            agent_mode: row.agent_mode,
            is_archived: row.is_archived,
            permission_mode,
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
    pub user_id: Option<Uuid>,
    pub metadata: Option<serde_json::Value>,
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
            user_id: row.user_id.map(UserId::from_uuid),
            metadata: row.metadata,
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
    pub owner_type: String,
    pub owner_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub conversation_id: Option<Uuid>,
    pub next_run_at: DateTime<Utc>,
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
            owner_type: row.owner_type,
            owner_id: row.owner_id,
            workspace_id: row.workspace_id,
            created_by: row.created_by,
            conversation_id: row.conversation_id,
            next_run_at: row.next_run_at,
            last_run_at: row.last_run_at,
            created_at: row.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct JobRunRow {
    pub id: Uuid,
    pub job_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: String,
    pub result: Vec<u8>,
    pub error: Option<String>,
}

impl From<JobRunRow> for JobRun {
    fn from(row: JobRunRow) -> Self {
        JobRun {
            id: JobRunId::from_uuid(row.id),
            job_id: JobId::from_uuid(row.job_id),
            started_at: row.started_at,
            finished_at: row.finished_at,
            status: row.status,
            result: row.result,
            error: row.error,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct WorkspaceRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub root_path: String,
    pub state: WorkspaceState,
    pub created_by: Uuid,
    pub archived_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<WorkspaceRow> for Workspace {
    fn from(row: WorkspaceRow) -> Self {
        Workspace {
            id: WorkspaceId::from_uuid(row.id),
            user_id: UserId::from_uuid(row.user_id),
            name: row.name,
            description: row.description,
            root_path: row.root_path,
            state: row.state,
            created_by: UserId::from_uuid(row.created_by),
            archived_at: row.archived_at,
            deleted_at: row.deleted_at,
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
    pub is_linked: bool,
    pub remote_url: Option<String>,
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
            is_linked: row.is_linked,
            remote_url: row.remote_url,
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
    pub state: WorktreeState,
    pub created_by: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub conversation_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
}

impl From<WorktreeRow> for Worktree {
    fn from(row: WorktreeRow) -> Self {
        Worktree {
            id: WorktreeId::from_uuid(row.id),
            repo_id: WorkspaceRepoId::from_uuid(row.repo_id),
            branch: row.branch,
            path: row.path,
            state: row.state,
            created_by: row.created_by.map(UserId::from_uuid),
            task_id: row.task_id,
            conversation_id: row.conversation_id.map(ConversationId::from_uuid),
            created_at: row.created_at,
            last_active_at: row.last_active_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct ArtifactRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub kind: ArtifactKind,
    pub state: ArtifactState,
    pub title: String,
    pub description: Option<String>,
    pub storage_type: String,
    pub git_repo: Option<String>,
    pub git_ref: Option<String>,
    pub blob_key: Option<String>,
    pub inline_content: Option<String>,
    pub created_by: Option<Uuid>,
    pub conversation_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub reviewed_by: Option<Uuid>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<ArtifactRow> for Artifact {
    fn from(row: ArtifactRow) -> Self {
        Artifact {
            id: ArtifactId::from_uuid(row.id),
            workspace_id: WorkspaceId::from_uuid(row.workspace_id),
            user_id: UserId::from_uuid(row.user_id),
            kind: row.kind,
            state: row.state,
            title: row.title,
            description: row.description,
            storage_type: row.storage_type,
            git_repo: row.git_repo,
            git_ref: row.git_ref,
            blob_key: row.blob_key,
            inline_content: row.inline_content,
            created_by: row.created_by.map(UserId::from_uuid),
            conversation_id: row.conversation_id.map(ConversationId::from_uuid),
            task_id: row.task_id,
            parent_id: row.parent_id.map(ArtifactId::from_uuid),
            reviewed_by: row.reviewed_by.map(UserId::from_uuid),
            reviewed_at: row.reviewed_at,
            metadata: row.metadata,
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

#[derive(sqlx::FromRow)]
pub(crate) struct EncryptionKeyRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub encrypted_dek: Vec<u8>,
    pub mek_version: i32,
    pub created_at: DateTime<Utc>,
    pub rotated_at: Option<DateTime<Utc>>,
}

impl From<EncryptionKeyRow> for StoredDek {
    fn from(row: EncryptionKeyRow) -> Self {
        StoredDek {
            id: EncryptionKeyId::from_uuid(row.id),
            user_id: UserId::from_uuid(row.user_id),
            encrypted_dek: row.encrypted_dek,
            mek_version: row.mek_version,
            created_at: row.created_at,
            rotated_at: row.rotated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct SecretMetadataRow {
    pub id: Uuid,
    pub name: String,
    pub secret_type: String,
    pub metadata: serde_json::Value,
    pub priority: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<SecretMetadataRow> for SecretMetadata {
    fn from(row: SecretMetadataRow) -> Self {
        SecretMetadata {
            id: SecretId::from_uuid(row.id),
            name: row.name,
            secret_type: row.secret_type,
            metadata: row.metadata,
            priority: row.priority,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct UserSecretRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub secret_type: String,
    pub metadata: serde_json::Value,
    pub encrypted_data: Vec<u8>,
    pub priority: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<UserSecretRow> for SecretRow {
    fn from(row: UserSecretRow) -> Self {
        SecretRow {
            id: SecretId::from_uuid(row.id),
            user_id: UserId::from_uuid(row.user_id),
            name: row.name,
            secret_type: row.secret_type,
            metadata: row.metadata,
            encrypted_data: row.encrypted_data,
            priority: row.priority,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// Row type for the conversation_users table.
#[derive(sqlx::FromRow)]
pub(crate) struct ConversationUserRow {
    pub conversation_id: Uuid,
    pub user_id: Uuid,
    pub unread_count: i32,
    pub last_read_at: Option<DateTime<Utc>>,
    pub role: ConversationUserRole,
    pub joined_at: DateTime<Utc>,
}

impl From<ConversationUserRow> for ConversationUser {
    fn from(row: ConversationUserRow) -> Self {
        ConversationUser {
            conversation_id: ConversationId::from_uuid(row.conversation_id),
            user_id: UserId::from_uuid(row.user_id),
            unread_count: row.unread_count,
            last_read_at: row.last_read_at,
            role: row.role,
            joined_at: row.joined_at,
        }
    }
}

/// Row type for conversation_users joined with users (for member lists).
#[derive(sqlx::FromRow)]
pub(crate) struct ConversationUserWithUsernameRow {
    pub conversation_id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub unread_count: i32,
    pub last_read_at: Option<DateTime<Utc>>,
    pub role: ConversationUserRole,
    pub joined_at: DateTime<Utc>,
}

impl From<ConversationUserWithUsernameRow> for ConversationUserWithUsername {
    fn from(row: ConversationUserWithUsernameRow) -> Self {
        ConversationUserWithUsername {
            conversation_id: ConversationId::from_uuid(row.conversation_id),
            user_id: UserId::from_uuid(row.user_id),
            username: row.username,
            unread_count: row.unread_count,
            last_read_at: row.last_read_at,
            role: row.role,
            joined_at: row.joined_at,
        }
    }
}

/// Row type for the tags table.
#[derive(sqlx::FromRow)]
pub(crate) struct TagRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub color: String,
    pub created_at: DateTime<Utc>,
}

impl From<TagRow> for Tag {
    fn from(row: TagRow) -> Self {
        Tag {
            id: TagId::from_uuid(row.id),
            user_id: UserId::from_uuid(row.user_id),
            name: row.name,
            color: row.color,
            created_at: row.created_at,
        }
    }
}

/// Row type for the message_tags join used by `list_by_conversation_messages`.
#[derive(sqlx::FromRow)]
pub(crate) struct MessageTagRow {
    pub message_id: Uuid,
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub color: String,
    pub created_at: DateTime<Utc>,
}

/// Row type for the conversation + unread_count join used by `list_with_details`.
#[derive(sqlx::FromRow)]
pub(crate) struct ConversationWithUnreadRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: Option<String>,
    pub workspace_id: Option<Uuid>,
    pub kind: ConversationKind,
    pub agent_mode: AgentMode,
    pub is_archived: bool,
    pub permission_mode: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub unread_count: i32,
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
            owner_type: "system".into(),
            owner_id: None,
            workspace_id: None,
            created_by: None,
            conversation_id: None,
            next_run_at: Utc::now(),
            last_run_at: None,
            created_at: Utc::now(),
        };
        let job: Job = row.into();
        assert_eq!(job.name, "test_job");
        assert_eq!(job.status, JobStatus::Active);
        assert_eq!(job.owner_type, "system");
    }
}
