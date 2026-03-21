//! Domain types and primitives shared across all crates.

pub mod access;
pub mod agent_repos;
pub mod api;
pub mod domain;
pub mod enums;
pub mod ids;
pub mod input;
pub mod job_payload;
pub mod repo;
pub mod tool;

pub use access::{CallerContext, Permission, TriggerKind};
pub use agent_repos::AgentRepos;
pub use api::ApiResponse;
pub use domain::{
    Artifact, AuditLogEntry, Conversation, ConversationUser, ConversationUserWithUsername,
    ConversationWithDetails, Job, JobRun, Message, Plugin, PluginAuditLog, Role, SecretMetadata,
    SecretRow, Session, StoredDek, Tag, User, UserRole, Workspace, WorkspaceRepoEntry, Worktree,
};
pub use enums::{
    AgentMode, ArtifactKind, ArtifactRelation, ArtifactState, ConversationKind,
    ConversationUserRole, JobStatus, MessageRole, PluginKind, PluginOrigin, PluginScope,
    PluginStatus, RoleKind, ScopeKind, UserStatus, WorkspaceState, WorktreeState,
};
pub use ids::{
    ArtifactId, AuditLogId, ConversationId, EncryptionKeyId, JobId, JobRunId, MessageId, PluginId,
    RoleId, ScopeId, SecretId, SessionId, TagId, ToolId, UserId, WorkspaceId, WorkspaceRepoId,
    WorktreeId,
};
pub use input::{
    ArtifactFilter, CreateArtifact, CreateAuditLog, CreateJob, CreateMessage, CreatePlugin,
    CreatePluginAuditLog, CreateSession, CreateTag, CreateUser, ListConversationsFilter, NewSecret,
    PluginFilter, RegisterRepo, UpdateSecret,
};
pub use job_payload::{ArtifactType, InternalOp, JobPayload};
pub use repo::{
    ArtifactRepo, AuditLogRepo, ConversationRepo, ConversationUserRepo, JobRepo, JobRunRepo,
    MessageRepo, PluginRepo, RoleRepo, SecretRepo, SessionRepo, TagRepo, UserRepo, WorkspaceRepo,
    WorkspaceRepoRepo, WorktreeRepo,
};
pub use tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
