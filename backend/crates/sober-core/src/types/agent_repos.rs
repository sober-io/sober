//! Trait bundle for all repository types needed by the agent.

use super::{
    ArtifactRepo, AuditLogRepo, ConversationRepo, EvolutionRepo, MessageRepo, PluginRepo,
    SecretRepo, ToolExecutionRepo, UserRepo, WorkspaceRepo, WorkspaceSettingsRepo,
};

/// Bundles all repository traits needed by the agent.
///
/// Avoids an unwieldy generic parameter list on `Agent<Msg, Conv, ...>`.
/// Production uses `PgAgentRepos`; tests can mock individual repos.
///
/// `Clone` bounds on `Secret`, `Audit`, and `Artifact` allow the agent to
/// create per-conversation tool contexts that hold their own repo handles.
pub trait AgentRepos: Send + Sync + 'static {
    type Msg: MessageRepo + Clone;
    type Conv: ConversationRepo;
    type User: UserRepo;
    type Secret: SecretRepo + Clone;
    type Audit: AuditLogRepo + Clone;
    type Artifact: ArtifactRepo + Clone;
    type Workspace: WorkspaceRepo;
    type Plg: PluginRepo;
    type ToolExec: ToolExecutionRepo;
    type WsSettings: WorkspaceSettingsRepo;
    type Evo: EvolutionRepo;

    fn messages(&self) -> &Self::Msg;
    fn conversations(&self) -> &Self::Conv;
    fn users(&self) -> &Self::User;
    fn secrets(&self) -> &Self::Secret;
    fn audit_log(&self) -> &Self::Audit;
    fn artifacts(&self) -> &Self::Artifact;
    fn workspaces(&self) -> &Self::Workspace;
    fn plugins(&self) -> &Self::Plg;
    fn tool_executions(&self) -> &Self::ToolExec;
    fn workspace_settings(&self) -> &Self::WsSettings;
    fn evolution(&self) -> &Self::Evo;
}
