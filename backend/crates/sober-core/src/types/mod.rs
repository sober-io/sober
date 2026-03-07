//! Domain types and primitives shared across all crates.

pub mod access;
pub mod api;
pub mod enums;
pub mod ids;
pub mod tool;

pub use access::{CallerContext, Permission, TriggerKind};
pub use api::ApiResponse;
pub use enums::{MessageRole, ScopeKind, UserStatus};
pub use ids::{
    ConversationId, McpServerId, MessageId, RoleId, ScopeId, SessionId, ToolId, UserId, WorkspaceId,
};
pub use tool::{Tool, ToolError, ToolMetadata, ToolOutput};
