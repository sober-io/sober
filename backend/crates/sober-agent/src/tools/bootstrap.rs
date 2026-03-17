//! Centralized tool construction for the agent.
//!
//! [`ToolBootstrap`] holds all dependencies needed to build tools and produces
//! a complete [`ToolRegistry`] for each conversation turn.  Static tools
//! (shell, search, memory) are rebuilt with per-turn context (e.g. workspace
//! directory).  Conversation-scoped tools (secrets, artifacts, snapshots) are
//! only added when the required context is available.

use std::path::PathBuf;
use std::sync::Arc;

use sober_core::config::MemoryConfig;
use sober_core::types::AgentRepos;
use sober_core::types::ids::{ConversationId, UserId, WorkspaceId};
use sober_core::types::tool::Tool;
use sober_crypto::envelope::Mek;
use sober_llm::LlmEngine;
use sober_memory::MemoryStore;
use sober_sandbox::{CommandPolicy, SandboxPolicy};
use sober_workspace::{BlobStore, SnapshotManager};

use super::shell::SharedPermissionMode;
use super::{
    ArtifactToolContext, CreateArtifactTool, CreateSnapshotTool, DeleteArtifactTool,
    DeleteSecretTool, FetchUrlTool, ListArtifactsTool, ListSecretsTool, ListSnapshotsTool,
    ReadArtifactTool, ReadSecretTool, RecallTool, RememberTool, RestoreSnapshotTool,
    SchedulerTools, SecretToolContext, ShellTool, SnapshotToolContext, StoreSecretTool,
    ToolRegistry, WebSearchTool,
};
use crate::SharedSchedulerClient;

/// Per-turn context that varies between conversations.
///
/// Passed to [`ToolBootstrap::build`] to produce a registry with the correct
/// workspace paths and conversation-scoped tools.
pub struct TurnContext {
    /// The authenticated user for this turn.
    pub user_id: UserId,
    /// The conversation being served.
    pub conversation_id: ConversationId,
    /// Workspace ID (from the conversation), if any.
    pub workspace_id: Option<WorkspaceId>,
    /// Resolved filesystem path for the conversation workspace directory.
    pub workspace_dir: Option<PathBuf>,
}

/// Centralized tool construction for the agent.
///
/// Holds all dependencies needed to build tools and produces a complete
/// [`ToolRegistry`] for each conversation turn. Static tools (shell, search,
/// memory) are rebuilt with per-turn context (e.g. workspace directory).
/// Conversation-scoped tools (secrets, artifacts, snapshots) are only added
/// when the required context is available.
pub struct ToolBootstrap<R: AgentRepos> {
    // -- Static tool dependencies --
    /// Policy for classifying and denying shell commands.
    pub command_policy: CommandPolicy,
    /// Shared permission mode (may be changed at runtime via gRPC).
    pub permission_mode: SharedPermissionMode,
    /// Default workspace root used when no per-conversation directory exists.
    pub default_workspace_root: PathBuf,
    /// Sandbox policy applied to shell executions.
    pub sandbox_policy: SandboxPolicy,
    /// SearXNG instance URL for web search.
    pub searxng_url: String,
    /// Shared handle to the scheduler gRPC client.
    pub scheduler_client: SharedSchedulerClient,
    /// Vector memory store for recall/remember tools.
    pub memory: Arc<MemoryStore>,
    /// LLM engine for embedding in recall/remember tools.
    pub llm: Arc<dyn LlmEngine>,
    /// Memory configuration (decay, boost, prune thresholds).
    pub memory_config: MemoryConfig,
    /// Whether the shell tool should auto-snapshot before dangerous commands.
    pub auto_snapshot: bool,
    /// Maximum number of snapshots retained per workspace.
    pub max_snapshots: Option<u32>,

    // -- Conversation-scoped tool dependencies --
    /// Repository bundle for creating per-conversation tool contexts.
    pub repos: Arc<R>,
    /// Master encryption key for secret tools (None = secrets disabled).
    pub mek: Option<Arc<Mek>>,
    /// Content-addressed blob store for artifact tools.
    pub blob_store: Arc<BlobStore>,
    /// Snapshot manager for snapshot and shell auto-snapshot tools.
    pub snapshot_manager: Arc<SnapshotManager>,
}

impl<R: AgentRepos> ToolBootstrap<R> {
    /// Builds a complete [`ToolRegistry`] for one conversation turn.
    ///
    /// 1. Creates the shell tool with the workspace directory as its home
    ///    (eliminating the `workdir_override` hack).
    /// 2. Creates web_search, fetch_url, scheduler, recall, remember tools.
    /// 3. If MEK is configured: adds secret tools (store, read, list, delete).
    /// 4. If workspace_dir is set: adds artifact and snapshot tools.
    pub fn build(&self, ctx: &TurnContext) -> ToolRegistry {
        let mut tools: Vec<Arc<dyn Tool>> = Vec::new();

        // 1. Shell tool — use workspace_dir when available, otherwise the default root.
        let shell_workspace = ctx
            .workspace_dir
            .clone()
            .unwrap_or_else(|| self.default_workspace_root.clone());
        let shell_tool = ShellTool::new(
            self.command_policy.clone(),
            Arc::clone(&self.permission_mode),
            shell_workspace,
            self.sandbox_policy.clone(),
            self.auto_snapshot,
            self.max_snapshots,
            Some((*self.snapshot_manager).clone()),
        );
        tools.push(Arc::new(shell_tool));

        // 2. Static tools (no per-conversation state needed).
        tools.push(Arc::new(WebSearchTool::new(self.searxng_url.clone())));
        tools.push(Arc::new(FetchUrlTool::new()));
        tools.push(Arc::new(SchedulerTools::new(Arc::clone(
            &self.scheduler_client,
        ))));
        tools.push(Arc::new(RecallTool::new(
            Arc::clone(&self.memory),
            Arc::clone(&self.llm),
            self.memory_config.clone(),
        )));
        tools.push(Arc::new(RememberTool::new(
            Arc::clone(&self.memory),
            Arc::clone(&self.llm),
            self.memory_config.clone(),
        )));

        // 3. Secret tools — available whenever a MEK is configured.
        if let Some(mek) = &self.mek {
            let secret_ctx = Arc::new(SecretToolContext {
                secret_repo: Arc::new(self.repos.secrets().clone()),
                audit_repo: Arc::new(self.repos.audit_log().clone()),
                mek: Arc::clone(mek),
                user_id: ctx.user_id,
                conversation_id: Some(ctx.conversation_id),
            });
            tools.push(Arc::new(StoreSecretTool::new(Arc::clone(&secret_ctx))));
            tools.push(Arc::new(ReadSecretTool::new(Arc::clone(&secret_ctx))));
            tools.push(Arc::new(ListSecretsTool::new(Arc::clone(&secret_ctx))));
            tools.push(Arc::new(DeleteSecretTool::new(secret_ctx)));
        }

        // 4. Artifact and snapshot tools — only when a workspace directory exists.
        if let (Some(ws_id), Some(ws_dir)) = (ctx.workspace_id, ctx.workspace_dir.as_ref()) {
            let artifact_ctx = Arc::new(ArtifactToolContext {
                artifact_repo: Arc::new(self.repos.artifacts().clone()),
                audit_repo: Arc::new(self.repos.audit_log().clone()),
                blob_store: Arc::clone(&self.blob_store),
                user_id: ctx.user_id,
                conversation_id: ctx.conversation_id,
                workspace_id: ws_id,
            });
            tools.push(Arc::new(CreateArtifactTool::new(Arc::clone(&artifact_ctx))));
            tools.push(Arc::new(ListArtifactsTool::new(Arc::clone(&artifact_ctx))));
            tools.push(Arc::new(ReadArtifactTool::new(Arc::clone(&artifact_ctx))));
            tools.push(Arc::new(DeleteArtifactTool::new(artifact_ctx)));

            let snapshot_ctx = Arc::new(SnapshotToolContext {
                artifact_repo: Arc::new(self.repos.artifacts().clone()),
                audit_repo: Arc::new(self.repos.audit_log().clone()),
                snapshot_manager: Arc::clone(&self.snapshot_manager),
                conversation_id: ctx.conversation_id,
                workspace_id: ws_id,
                conversation_dir: ws_dir.clone(),
            });
            tools.push(Arc::new(CreateSnapshotTool::new(Arc::clone(&snapshot_ctx))));
            tools.push(Arc::new(ListSnapshotsTool::new(Arc::clone(&snapshot_ctx))));
            tools.push(Arc::new(RestoreSnapshotTool::new(snapshot_ctx)));
        }

        ToolRegistry::with_builtins(tools)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_context_fields() {
        let ctx = TurnContext {
            user_id: UserId::new(),
            conversation_id: ConversationId::new(),
            workspace_id: None,
            workspace_dir: None,
        };
        // Sanity: context is constructible without workspace.
        assert!(ctx.workspace_id.is_none());
        assert!(ctx.workspace_dir.is_none());
    }
}
