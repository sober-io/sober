//! Trait bundle for all repository types needed by the agent.

use super::{
    ArtifactRepo, AuditLogRepo, ConversationRepo, McpServerRepo, MessageRepo, SecretRepo, UserRepo,
    WorkspaceRepo,
};

/// Bundles all repository traits needed by the agent.
///
/// Avoids an unwieldy generic parameter list on `Agent<Msg, Conv, Mcp, ...>`.
/// Production uses `PgAgentRepos`; tests can mock individual repos.
pub trait AgentRepos: Send + Sync + 'static {
    type Msg: MessageRepo;
    type Conv: ConversationRepo;
    type Mcp: McpServerRepo;
    type User: UserRepo;
    type Secret: SecretRepo;
    type Audit: AuditLogRepo;
    type Artifact: ArtifactRepo;
    type Workspace: WorkspaceRepo;

    fn messages(&self) -> &Self::Msg;
    fn conversations(&self) -> &Self::Conv;
    fn mcp_servers(&self) -> &Self::Mcp;
    fn users(&self) -> &Self::User;
    fn secrets(&self) -> &Self::Secret;
    fn audit_log(&self) -> &Self::Audit;
    fn artifacts(&self) -> &Self::Artifact;
    fn workspaces(&self) -> &Self::Workspace;
}
