//! Domain types and primitives shared across all crates.

pub mod access;
pub mod api;
pub mod domain;
pub mod enums;
pub mod ids;
pub mod input;
pub mod repo;
pub mod tool;

pub use access::{CallerContext, Permission, TriggerKind};
pub use api::ApiResponse;
pub use domain::{
    Artifact, AuditLogEntry, Conversation, Job, McpServerConfig, Message, Role, SecretMetadata,
    SecretRow, SecretScope, Session, StoredDek, User, UserRole, Workspace, WorkspaceRepoEntry,
    Worktree,
};
pub use enums::{
    ArtifactKind, ArtifactRelation, ArtifactState, JobStatus, MessageRole, RoleKind, ScopeKind,
    UserStatus,
};
pub use ids::{
    ArtifactId, AuditLogId, ConversationId, EncryptionKeyId, JobId, McpServerId, MessageId, RoleId,
    ScopeId, SecretId, SessionId, ToolId, UserId, WorkspaceId, WorkspaceRepoId, WorktreeId,
};
pub use input::{
    ArtifactFilter, CreateArtifact, CreateAuditLog, CreateJob, CreateMcpServer, CreateMessage,
    CreateSession, CreateUser, NewSecret, RegisterRepo, UpdateMcpServer, UpdateSecret,
};
pub use repo::{
    ArtifactRepo, AuditLogRepo, ConversationRepo, JobRepo, McpServerRepo, MessageRepo, RoleRepo,
    SecretRepo, SessionRepo, UserRepo, WorkspaceRepo, WorkspaceRepoRepo, WorktreeRepo,
};
pub use tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
