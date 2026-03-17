//! Domain types and primitives shared across all crates.

pub mod access;
pub mod api;
pub mod domain;
pub mod enums;
pub mod ids;
pub mod input;
pub mod job_payload;
pub mod repo;
pub mod tool;

pub use access::{CallerContext, Permission, TriggerKind};
pub use api::ApiResponse;
pub use domain::{
    Artifact, AuditLogEntry, Conversation, ConversationUser, ConversationUserWithUsername,
    ConversationWithDetails, Job, JobRun, McpServerConfig, Message, Role, SecretMetadata,
    SecretRow, Session, StoredDek, Tag, User, UserRole, Workspace, WorkspaceRepoEntry, Worktree,
};
pub use enums::{
    AgentMode, ArtifactKind, ArtifactRelation, ArtifactState, ConversationKind,
    ConversationUserRole, JobStatus, MessageRole, RoleKind, ScopeKind, UserStatus, WorkspaceState,
    WorktreeState,
};
pub use ids::{
    ArtifactId, AuditLogId, ConversationId, EncryptionKeyId, JobId, JobRunId, McpServerId,
    MessageId, RoleId, ScopeId, SecretId, SessionId, TagId, ToolId, UserId, WorkspaceId,
    WorkspaceRepoId, WorktreeId,
};
pub use input::{
    ArtifactFilter, CreateArtifact, CreateAuditLog, CreateJob, CreateMcpServer, CreateMessage,
    CreateSession, CreateTag, CreateUser, ListConversationsFilter, NewSecret, RegisterRepo,
    UpdateMcpServer, UpdateSecret,
};
pub use job_payload::{ArtifactType, InternalOp, JobPayload};
pub use repo::{
    ArtifactRepo, AuditLogRepo, ConversationRepo, ConversationUserRepo, JobRepo, JobRunRepo,
    McpServerRepo, MessageRepo, RoleRepo, SecretRepo, SessionRepo, TagRepo, UserRepo,
    WorkspaceRepo, WorkspaceRepoRepo, WorktreeRepo,
};
pub use tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
