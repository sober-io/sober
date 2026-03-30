//! Centralized tool construction for the agent.
//!
//! [`ToolBootstrap`] holds all dependencies needed to build tools and produces
//! a complete [`ToolRegistry`] for each conversation turn. Tool-specific
//! configuration is grouped into dedicated config structs ([`ShellToolConfig`],
//! [`SearchToolConfig`], [`MemoryToolConfig`]) so that adding new tools does
//! not bloat the top-level struct.
//!
//! Static tools (web_search, fetch_url, scheduler, recall, remember) are built
//! once at startup via [`ToolBootstrap::build_static_tools`] and reused across
//! turns.  Per-conversation tools (shell, secrets, artifacts, snapshots) are
//! rebuilt each turn by [`ToolBootstrap::build`].

use std::path::PathBuf;
use std::sync::Arc;

use sober_core::config::MemoryConfig;
use sober_core::types::AgentRepos;
use sober_core::types::domain::WorkspaceSettings;
use sober_core::types::ids::{ConversationId, UserId, WorkspaceId};
use sober_core::types::tool::Tool;
use sober_crypto::envelope::Mek;
use sober_db::PgSandboxExecutionLogRepo;
use sober_llm::LlmEngine;
use sober_memory::MemoryStore;
use sober_plugin::PluginManager;
use sober_plugin_gen::PluginGenerator;
use sober_sandbox::{CommandPolicy, NetMode, SandboxPolicy, SandboxProfile};
use sober_skill::SkillActivationState;
use sober_workspace::{BlobStore, SnapshotManager};

use super::shell::SharedPermissionMode;
use super::{
    ArtifactToolContext, CreateArtifactTool, CreateSnapshotTool, DeleteArtifactTool,
    DeleteSecretTool, FetchUrlTool, GeneratePluginConfig, GeneratePluginTool, ListArtifactsTool,
    ListSecretsTool, ListSnapshotsTool, ProposeAutomationTool, ProposeInstructionTool,
    ProposeSkillTool, ProposeToolTool, ReadArtifactTool, ReadSecretTool, RecallTool, RememberTool,
    RestoreSnapshotTool, SchedulerTools, SecretToolContext, ShellTool, SnapshotToolContext,
    StoreSecretTool, ToolRegistry, WebSearchTool,
};
use crate::SharedSchedulerClient;

// ---------------------------------------------------------------------------
// Tool-specific configuration structs
// ---------------------------------------------------------------------------

/// Configuration for the shell tool.
pub struct ShellToolConfig {
    /// Policy for classifying and denying shell commands.
    pub command_policy: CommandPolicy,
    /// Shared permission mode (may be changed at runtime via gRPC).
    pub permission_mode: SharedPermissionMode,
    /// Default workspace root used when no per-conversation directory exists.
    pub default_workspace_root: PathBuf,
    /// Sandbox policy applied to shell executions.
    pub sandbox_policy: SandboxPolicy,
    /// Whether the shell tool should auto-snapshot before dangerous commands.
    pub auto_snapshot: bool,
    /// Maximum number of snapshots retained per workspace.
    pub max_snapshots: Option<u32>,
    /// Sandbox execution log repo for persisting audit entries.
    pub sandbox_log_repo: Option<Arc<PgSandboxExecutionLogRepo>>,
}

/// Configuration for the web search tool.
pub struct SearchToolConfig {
    /// SearXNG instance URL.
    pub searxng_url: String,
}

/// Configuration for the memory tools (recall/remember).
pub struct MemoryToolConfig {
    /// Vector memory store.
    pub memory: Arc<MemoryStore>,
    /// LLM engine for embedding.
    pub llm: Arc<dyn LlmEngine>,
    /// Memory configuration (decay, boost, prune thresholds).
    pub config: MemoryConfig,
}

// ---------------------------------------------------------------------------
// TurnContext
// ---------------------------------------------------------------------------

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
    /// Workspace settings loaded from the DB (None if no workspace exists).
    pub workspace_settings: Option<WorkspaceSettings>,
    /// Tracks which skills have already been activated in this conversation.
    /// Prevents the same skill from being injected twice across multiple turns.
    /// NOT the skill cache — that's managed by [`SkillLoader`]'s TTL cache.
    pub skill_activation_state: Option<Arc<std::sync::Mutex<SkillActivationState>>>,
}

// ---------------------------------------------------------------------------
// ToolBootstrap
// ---------------------------------------------------------------------------

/// Centralized tool construction for the agent.
///
/// Holds all dependencies needed to build tools and produces a complete
/// [`ToolRegistry`] for each conversation turn. Static tools are built once
/// via [`build_static_tools`](Self::build_static_tools) and cached by the
/// caller. Per-conversation tools (shell, secrets, artifacts, snapshots) are
/// rebuilt each turn by [`build`](Self::build).
pub struct ToolBootstrap<R: AgentRepos> {
    // -- Tool configurations --
    /// Shell tool configuration.
    pub shell: ShellToolConfig,
    /// Web search tool configuration.
    pub search: SearchToolConfig,
    /// Memory tools (recall/remember) configuration.
    pub memory_tools: MemoryToolConfig,
    /// Shared handle to the scheduler gRPC client.
    pub scheduler_client: SharedSchedulerClient,

    // -- Shared dependencies for conversation-scoped tools --
    /// Repository bundle for creating per-conversation tool contexts.
    pub repos: Arc<R>,
    /// Master encryption key for secret tools (None = secrets disabled).
    pub mek: Option<Arc<Mek>>,
    /// Content-addressed blob store for artifact tools.
    pub blob_store: Arc<BlobStore>,
    /// Snapshot manager for snapshot and shell auto-snapshot tools.
    pub snapshot_manager: Arc<SnapshotManager>,
    /// Unified plugin manager for MCP, Skill, and WASM plugin tools.
    pub plugin_manager: Arc<PluginManager<R::Plg>>,
    /// LLM-powered plugin generator (None = generation disabled).
    pub plugin_generator: Option<Arc<PluginGenerator>>,
    /// Evolution configuration (detection limits, LLM params).
    pub evolution_config: sober_core::config::EvolutionConfig,
}

impl<R: AgentRepos> ToolBootstrap<R> {
    /// Builds static tools that do not change between conversations.
    ///
    /// Call once at startup and pass the result into [`build`](Self::build)
    /// on each turn to avoid re-creating identical tool instances.
    pub fn build_static_tools(&self) -> Vec<Arc<dyn Tool>> {
        vec![
            Arc::new(WebSearchTool::new(self.search.searxng_url.clone())),
            Arc::new(FetchUrlTool::new()),
            Arc::new(SchedulerTools::new(Arc::clone(&self.scheduler_client))),
            Arc::new(RecallTool::new(
                Arc::clone(&self.memory_tools.memory),
                Arc::clone(&self.memory_tools.llm),
                self.memory_tools.config.clone(),
            )),
            Arc::new(RememberTool::new(
                Arc::clone(&self.memory_tools.memory),
                Arc::clone(&self.memory_tools.llm),
                self.memory_tools.config.clone(),
            )),
            // Evolution proposal tools — static since they only need DB access.
            Arc::new(ProposeToolTool::new(Arc::clone(&self.repos))),
            Arc::new(ProposeSkillTool::new(Arc::clone(&self.repos))),
            Arc::new(ProposeInstructionTool::new(Arc::clone(&self.repos))),
            Arc::new(ProposeAutomationTool::new(Arc::clone(&self.repos))),
        ]
    }

    /// Returns `(name, description)` pairs for all built-in tools.
    ///
    /// Includes both static tools (always available) and per-turn tools
    /// (conditionally available depending on workspace/config). Used by the
    /// `ListTools` gRPC RPC to provide the frontend with a complete catalog.
    pub fn builtin_tool_names(&self) -> Vec<(String, String)> {
        let mut names: Vec<(String, String)> = self
            .build_static_tools()
            .into_iter()
            .map(|t| {
                let meta = t.metadata();
                (meta.name, meta.description)
            })
            .collect();

        // Per-turn tools that may be available depending on workspace/config.
        // Shell is always available; others depend on MEK, workspace, etc.
        names.push((
            "shell".into(),
            "Execute shell commands in a sandboxed environment".into(),
        ));

        if self.mek.is_some() {
            names.push(("store_secret".into(), "Store an encrypted secret".into()));
            names.push(("read_secret".into(), "Read a decrypted secret value".into()));
            names.push(("list_secrets".into(), "List stored secret names".into()));
            names.push(("delete_secret".into(), "Delete a stored secret".into()));
        }

        // Artifact and snapshot tools are available when a workspace exists.
        names.push((
            "create_artifact".into(),
            "Create a workspace artifact".into(),
        ));
        names.push(("list_artifacts".into(), "List workspace artifacts".into()));
        names.push(("read_artifact".into(), "Read a workspace artifact".into()));
        names.push((
            "delete_artifact".into(),
            "Delete a workspace artifact".into(),
        ));
        names.push(("snapshot".into(), "Create a workspace snapshot".into()));
        names.push(("list_snapshots".into(), "List workspace snapshots".into()));
        names.push((
            "restore_snapshot".into(),
            "Restore a workspace snapshot".into(),
        ));

        if self.plugin_generator.is_some() {
            names.push((
                "generate_plugin".into(),
                "Generate a new plugin via LLM".into(),
            ));
        }

        names
    }

    /// Builds a complete [`ToolRegistry`] for one conversation turn.
    ///
    /// Reuses the pre-built `static_tools` and adds per-conversation tools:
    ///
    /// 1. Shell tool with the correct workspace directory.
    /// 2. Secret tools (if MEK is configured).
    /// 3. Artifact and snapshot tools (if a workspace directory exists).
    /// 4. Skill tool (if any skills are available in the catalog).
    pub async fn build(&self, ctx: &TurnContext, static_tools: &[Arc<dyn Tool>]) -> ToolRegistry {
        let mut tools: Vec<Arc<dyn Tool>> = static_tools.to_vec();

        // 1. Shell tool — use workspace_dir when available, otherwise the default root.
        //    If workspace settings exist, resolve sandbox policy from them.
        let shell_workspace = ctx
            .workspace_dir
            .clone()
            .unwrap_or_else(|| self.shell.default_workspace_root.clone());

        let resolved_config = if let Some(ref ws_settings) = ctx.workspace_settings {
            let mut policy = resolve_sandbox_policy(ws_settings, &self.shell.sandbox_policy);
            // Ensure the workspace directory is in the policy's read/write paths.
            if let Some(ref ws_dir) = ctx.workspace_dir {
                if !policy.fs_read.contains(ws_dir) {
                    policy.fs_read.push(ws_dir.clone());
                }
                if !policy.fs_write.contains(ws_dir) {
                    policy.fs_write.push(ws_dir.clone());
                }
            }
            // Apply workspace-level permission mode to the shared lock so the
            // shell tool (and any runtime readers) use the correct mode.
            {
                let mut guard = self
                    .shell
                    .permission_mode
                    .write()
                    .expect("permission mode lock poisoned");
                *guard = ws_settings.permission_mode;
            }
            ShellToolConfig {
                command_policy: self.shell.command_policy.clone(),
                permission_mode: Arc::clone(&self.shell.permission_mode),
                default_workspace_root: self.shell.default_workspace_root.clone(),
                sandbox_policy: policy,
                auto_snapshot: ws_settings.auto_snapshot,
                max_snapshots: ws_settings.max_snapshots.map(|n| n as u32),
                sandbox_log_repo: self.shell.sandbox_log_repo.clone(),
            }
        } else {
            // No workspace settings — use startup defaults.
            ShellToolConfig {
                command_policy: self.shell.command_policy.clone(),
                permission_mode: Arc::clone(&self.shell.permission_mode),
                default_workspace_root: self.shell.default_workspace_root.clone(),
                sandbox_policy: self.shell.sandbox_policy.clone(),
                auto_snapshot: self.shell.auto_snapshot,
                max_snapshots: self.shell.max_snapshots,
                sandbox_log_repo: self.shell.sandbox_log_repo.clone(),
            }
        };

        let shell_tool = ShellTool::new(
            &resolved_config,
            shell_workspace,
            Some((*self.snapshot_manager).clone()),
            Some(ctx.user_id),
            ctx.workspace_id,
        );
        tools.push(Arc::new(shell_tool));

        // 2. Secret tools — available whenever a MEK is configured.
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

        // 3. Artifact and snapshot tools — only when a workspace directory exists.
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

        // 4. Plugin tools — MCP, Skill, and WASM tools from PluginManager.
        //    Reuses the per-conversation activation state when provided so that
        //    skills activated in earlier turns remain marked as active.
        let user_home = sober_workspace::user_home_dir();
        let workspace_path = ctx.workspace_dir.clone().unwrap_or_default();
        let disabled_plugins = ctx
            .workspace_settings
            .as_ref()
            .map(|s| s.disabled_plugins.as_slice())
            .unwrap_or_default();
        match self
            .plugin_manager
            .tools_for_turn(
                ctx.user_id,
                &user_home,
                &workspace_path,
                ctx.workspace_id,
                ctx.skill_activation_state.clone(),
                disabled_plugins,
            )
            .await
        {
            Ok(plugin_tools) => {
                // Safety net: plugins installed via the gRPC InstallPlugin
                // handler bypass the audit pipeline, so reserved-name
                // rejection doesn't cover them. Drop collisions at runtime.
                for tool in plugin_tools {
                    let name = tool.metadata().name;
                    if tools.iter().any(|t| t.metadata().name == name) {
                        tracing::warn!(
                            tool_name = %name,
                            "plugin tool collides with built-in, skipping"
                        );
                    } else {
                        tools.push(tool);
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to load plugin tools, continuing without them");
            }
        }

        // 5. Generate-plugin tool — available when a plugin generator is configured.
        if let Some(generator) = &self.plugin_generator {
            // Provide the artifact repo when a workspace context is active so that
            // generated WASM binaries are tracked as workspace artifacts.
            let artifact_repo: Option<Arc<R::Artifact>> = ctx
                .workspace_id
                .map(|_| Arc::new(self.repos.artifacts().clone()));
            tools.push(Arc::new(GeneratePluginTool::new(
                Arc::clone(&self.plugin_manager),
                GeneratePluginConfig {
                    generator: Arc::clone(generator),
                    blob_store: Arc::clone(&self.blob_store),
                    artifact_repo,
                    workspace_dir: ctx.workspace_dir.clone(),
                    workspace_id: ctx.workspace_id,
                    conversation_id: Some(ctx.conversation_id),
                    user_id: ctx.user_id,
                    reserved_tool_names: tools.iter().map(|t| t.metadata().name).collect(),
                },
            )));
        }

        // 6. Capability filtering — remove tools disabled by workspace settings.
        if let Some(ref settings) = ctx.workspace_settings
            && !settings.disabled_tools.is_empty()
        {
            tools.retain(|tool| !settings.disabled_tools.contains(&tool.metadata().name));
        }

        ToolRegistry::with_builtins(tools)
    }
}

/// Resolves a [`SandboxPolicy`] from workspace settings.
///
/// Starts with the profile specified in settings (falling back to `standard`
/// for unknown names), then applies any non-null overrides from the settings.
fn resolve_sandbox_policy(settings: &WorkspaceSettings, fallback: &SandboxPolicy) -> SandboxPolicy {
    let profile = match settings.sandbox_profile.as_str() {
        "locked_down" => SandboxProfile::LockedDown,
        "standard" => SandboxProfile::Standard,
        "unrestricted" => SandboxProfile::Unrestricted,
        custom => SandboxProfile::Custom(custom.to_owned()),
    };

    // Resolve profile. For built-in profiles we pass an empty map since they
    // don't need custom lookup. For unknown custom profiles, fall back to the
    // startup default policy.
    let empty = std::collections::HashMap::new();
    let mut policy = profile.resolve(&empty).unwrap_or_else(|_| {
        tracing::warn!(
            profile = %settings.sandbox_profile,
            "unknown sandbox profile, falling back to startup default"
        );
        fallback.clone()
    });

    // Apply non-null overrides from workspace settings.
    if let Some(net_mode) = settings.sandbox_net_mode {
        use sober_core::types::SandboxNetMode;
        policy.net_mode = match net_mode {
            SandboxNetMode::None => NetMode::None,
            SandboxNetMode::AllowedDomains => {
                let domains = settings.sandbox_allowed_domains.clone().unwrap_or_default();
                NetMode::AllowedDomains(domains)
            }
            SandboxNetMode::Full => NetMode::Full,
        };
    }

    if let Some(seconds) = settings.sandbox_max_execution_seconds {
        policy.max_execution_seconds = seconds as u32;
    }

    if let Some(allow_spawn) = settings.sandbox_allow_spawn {
        policy.allow_spawn = allow_spawn;
    }

    policy
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
            workspace_settings: None,
            skill_activation_state: None,
        };
        assert!(ctx.workspace_id.is_none());
        assert!(ctx.workspace_dir.is_none());
        assert!(ctx.workspace_settings.is_none());
        assert!(ctx.skill_activation_state.is_none());
    }

    #[test]
    fn resolve_sandbox_policy_standard_profile() {
        let settings = WorkspaceSettings {
            workspace_id: WorkspaceId::new(),
            permission_mode: sober_core::types::PermissionMode::PolicyBased,
            auto_snapshot: true,
            max_snapshots: None,
            sandbox_profile: "standard".into(),
            sandbox_net_mode: None,
            sandbox_allowed_domains: None,
            sandbox_max_execution_seconds: None,
            sandbox_allow_spawn: None,
            disabled_tools: vec![],
            disabled_plugins: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let fallback = SandboxProfile::Standard
            .resolve(&Default::default())
            .unwrap();
        let policy = resolve_sandbox_policy(&settings, &fallback);
        assert_eq!(policy.max_execution_seconds, 60);
        assert!(!policy.allow_spawn);
    }

    #[test]
    fn resolve_sandbox_policy_with_overrides() {
        let settings = WorkspaceSettings {
            workspace_id: WorkspaceId::new(),
            permission_mode: sober_core::types::PermissionMode::Autonomous,
            auto_snapshot: false,
            max_snapshots: Some(5),
            sandbox_profile: "locked_down".into(),
            sandbox_net_mode: Some(sober_core::types::SandboxNetMode::Full),
            sandbox_allowed_domains: None,
            sandbox_max_execution_seconds: Some(120),
            sandbox_allow_spawn: Some(true),
            disabled_tools: vec![],
            disabled_plugins: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let fallback = SandboxProfile::Standard
            .resolve(&Default::default())
            .unwrap();
        let policy = resolve_sandbox_policy(&settings, &fallback);
        assert_eq!(policy.max_execution_seconds, 120);
        assert!(policy.allow_spawn);
        assert!(matches!(policy.net_mode, NetMode::Full));
    }

    #[test]
    fn resolve_sandbox_policy_unknown_profile_falls_back() {
        let settings = WorkspaceSettings {
            workspace_id: WorkspaceId::new(),
            permission_mode: sober_core::types::PermissionMode::PolicyBased,
            auto_snapshot: true,
            max_snapshots: None,
            sandbox_profile: "my_custom_profile".into(),
            sandbox_net_mode: None,
            sandbox_allowed_domains: None,
            sandbox_max_execution_seconds: None,
            sandbox_allow_spawn: None,
            disabled_tools: vec![],
            disabled_plugins: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let fallback = SandboxProfile::Standard
            .resolve(&Default::default())
            .unwrap();
        let policy = resolve_sandbox_policy(&settings, &fallback);
        // Should fall back to the startup policy.
        assert_eq!(policy.max_execution_seconds, fallback.max_execution_seconds);
    }
}
