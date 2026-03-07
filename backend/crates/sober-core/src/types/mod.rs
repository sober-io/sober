//! Domain types and primitives shared across all crates.

pub mod access;
pub mod api;
pub mod domain;
pub mod enums;
pub mod ids;
pub mod input;
pub mod tool;

pub use access::{CallerContext, Permission, TriggerKind};
pub use api::ApiResponse;
pub use domain::{
    Artifact, AuditLogEntry, Conversation, Job, McpServerConfig, Message, Role, Session, User,
    UserRole, Workspace, WorkspaceRepoEntry, Worktree,
};
pub use input::{
    ArtifactFilter, CreateArtifact, CreateAuditLog, CreateJob, CreateMcpServer, CreateMessage,
    CreateSession, CreateUser, RegisterRepo, UpdateMcpServer,
};
pub use enums::{
    ArtifactKind, ArtifactRelation, ArtifactState, JobStatus, MessageRole, ScopeKind, UserStatus,
};
pub use ids::{
    ArtifactId, AuditLogId, ConversationId, JobId, McpServerId, MessageId, RoleId, ScopeId,
    SessionId, ToolId, UserId, WorkspaceId, WorkspaceRepoId, WorktreeId,
};
pub use tool::{Tool, ToolError, ToolMetadata, ToolOutput};
