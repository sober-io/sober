//! Domain types and primitives shared across all crates.

pub mod access;
pub mod api;
pub mod enums;
pub mod ids;
pub mod tool;

pub use access::{CallerContext, Permission, TriggerKind};
pub use api::ApiResponse;
pub use enums::{
    ArtifactKind, ArtifactRelation, ArtifactState, JobStatus, MessageRole, ScopeKind, UserStatus,
};
pub use ids::{
    ArtifactId, AuditLogId, ConversationId, JobId, McpServerId, MessageId, RoleId, ScopeId,
    SessionId, ToolId, UserId, WorkspaceId, WorkspaceRepoId, WorktreeId,
};
pub use tool::{Tool, ToolError, ToolMetadata, ToolOutput};
