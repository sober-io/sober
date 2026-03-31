//! Domain types and primitives shared across all crates.

pub mod access;
pub mod agent_repos;
pub mod api;
pub mod content;
pub mod domain;
pub mod enums;
pub mod ids;
pub mod input;
pub mod job_payload;
pub mod repo;
pub mod tool;
pub mod tool_execution;

pub use access::{CallerContext, Permission, TriggerKind};
pub use agent_repos::AgentRepos;
pub use api::ApiResponse;
pub use content::ContentBlock;
pub use domain::{
    Artifact, AuditLogEntry, Conversation, ConversationAttachment, ConversationUser,
    ConversationUserWithUsername, ConversationWithDetails, EvolutionConfigRow, EvolutionEvent, Job,
    JobRun, Message, MessageSearchHit, Observation, Plugin, PluginAuditLog, Role, SecretMetadata,
    SecretRow, Session, StoredDek, Tag, User, UserRole, Workspace, WorkspaceAgentState,
    WorkspaceRepoEntry, WorkspaceSettings, Worktree,
};
pub use enums::{
    AgentMode, ArtifactKind, ArtifactRelation, ArtifactState, AttachmentKind, AutonomyLevel,
    ConversationKind, ConversationUserRole, EvolutionStatus, EvolutionType, JobStatus, MessageRole,
    PermissionMode, PluginKind, PluginOrigin, PluginScope, PluginStatus, RoleKind, SandboxNetMode,
    ScopeKind, ToolExecutionSource, ToolExecutionStatus, UserStatus, WorkspaceState, WorktreeState,
};
pub use ids::{
    ArtifactId, AuditLogId, ConversationAttachmentId, ConversationId, EncryptionKeyId,
    EvolutionEventId, JobId, JobRunId, MessageId, PluginId, RoleId, ScopeId, SecretId, SessionId,
    TagId, ToolExecutionId, ToolId, UserId, WorkspaceId, WorkspaceRepoId, WorktreeId,
};
pub use input::{
    ArtifactFilter, CreateArtifact, CreateAuditLog, CreateConversationAttachment,
    CreateEvolutionEvent, CreateJob, CreateMessage, CreatePlugin, CreatePluginAuditLog,
    CreatePluginExecutionLog, CreateSandboxExecutionLog, CreateSession, CreateTag, CreateUser,
    ListConversationsFilter, NewSecret, PluginFilter, RegisterRepo, UpdateSecret,
};
pub use job_payload::{ArtifactType, InternalOp, JobPayload};
pub use repo::{
    ArtifactRepo, AuditLogRepo, BlobGcRepo, ConversationAttachmentRepo, ConversationRepo,
    ConversationUserRepo, EvolutionRepo, JobRepo, JobRunRepo, MessageRepo, PluginExecutionLogRepo,
    PluginRepo, RoleRepo, SandboxExecutionLogRepo, SecretRepo, SessionRepo, TagRepo,
    ToolExecutionRepo, UserRepo, WorkspaceRepo, WorkspaceRepoRepo, WorkspaceSettingsRepo,
    WorktreeRepo,
};
pub use tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput, ToolVisibility};
pub use tool_execution::{CreateToolExecution, MessageWithExecutions, ToolExecution};
