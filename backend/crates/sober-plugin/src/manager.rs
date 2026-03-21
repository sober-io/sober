//! Unified plugin manager — wraps MCP, Skill, and WASM subsystems.
//!
//! [`PluginManager`] provides a single entry point for collecting tools from
//! all plugin kinds.  It queries the [`PluginRepo`] for enabled plugins,
//! delegates to [`McpPool`] for MCP servers, [`SkillLoader`] for filesystem
//! skills, and cached [`PluginHost`] instances for WASM plugins.
//!
//! # MCP connection lifecycle
//!
//! MCP server connections are managed externally (typically by the agent at
//! startup) via [`McpPool::connect_servers`].  The manager reads tool adapters
//! from the pool but does not own the connection lifecycle — this avoids a
//! dependency on `sober-sandbox` (required only for spawning MCP processes).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use sober_core::types::domain::Plugin;
use sober_core::types::enums::{PluginKind, PluginStatus};
use sober_core::types::ids::{PluginId, UserId};
use sober_core::types::input::PluginFilter;
use sober_core::types::repo::PluginRepo;
use sober_core::types::tool::Tool;
use sober_mcp::McpPool;
use sober_skill::{ActivateSkillTool, SkillActivationState, SkillLoader};
use tracing::{debug, warn};

use crate::error::PluginError;
use crate::host::PluginHost;
use crate::manifest::PluginManifest;
use crate::registry::{InstallRequest, PluginRegistry};
use crate::tool::PluginTool;

/// Unified plugin manager that collects tools from MCP, Skill, and WASM plugins.
///
/// Generic over the [`PluginRepo`] implementation for testability.  The manager
/// holds an [`McpPool`] (behind a tokio mutex for `&self` async methods),
/// a shared [`SkillLoader`], and a WASM host cache.
///
/// MCP server connections must be established externally via
/// [`McpPool::connect_servers`] before calling [`tools_for_turn`](Self::tools_for_turn).
/// This keeps sandbox-related concerns out of the plugin crate.
pub struct PluginManager<R: PluginRepo> {
    registry: PluginRegistry<R>,
    mcp_pool: tokio::sync::Mutex<McpPool>,
    skill_loader: Arc<SkillLoader>,
    /// WASM hosts cached by plugin ID.  Uses `std::sync::RwLock` because
    /// accesses are brief and never held across `.await` points.
    wasm_hosts: RwLock<HashMap<PluginId, Arc<Mutex<PluginHost>>>>,
}

impl<R: PluginRepo> PluginManager<R> {
    /// Creates a new plugin manager.
    ///
    /// The `mcp_pool` should be pre-connected to any MCP servers the user has
    /// configured.  The manager will query tools from connected servers but
    /// will not initiate new connections.
    pub fn new(plugin_repo: R, mcp_pool: McpPool, skill_loader: Arc<SkillLoader>) -> Self {
        Self {
            registry: PluginRegistry::new(plugin_repo),
            mcp_pool: tokio::sync::Mutex::new(mcp_pool),
            skill_loader,
            wasm_hosts: RwLock::new(HashMap::new()),
        }
    }

    /// Returns a reference to the plugin registry.
    pub fn registry(&self) -> &PluginRegistry<R> {
        &self.registry
    }

    /// Returns all tools for a single conversation turn.
    ///
    /// Queries enabled plugins owned by `user_id`, builds tools from each
    /// plugin kind, and appends the skill activation tool if skills are
    /// available.
    ///
    /// `user_home` is the user's home directory (e.g. from
    /// `sober_workspace::user_home_dir()`), used for skill discovery.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError`] if the plugin list query fails.  Individual
    /// plugin tool loading failures are logged and skipped so that one
    /// broken plugin does not prevent the rest from loading.
    pub async fn tools_for_turn(
        &self,
        user_id: UserId,
        user_home: &Path,
        workspace_dir: &Path,
        workspace_id: Option<sober_core::types::WorkspaceId>,
        skill_activation_state: Option<Arc<Mutex<SkillActivationState>>>,
    ) -> Result<Vec<Arc<dyn Tool>>, PluginError> {
        let filter = PluginFilter {
            status: Some(PluginStatus::Enabled),
            owner_id: Some(user_id),
            ..Default::default()
        };
        let plugins = self.registry.list(filter).await?;

        let mut tools: Vec<Arc<dyn Tool>> = Vec::new();

        for plugin in &plugins {
            let result = match plugin.kind {
                PluginKind::Mcp => self.mcp_tools(plugin).await,
                PluginKind::Skill => {
                    // Skills are loaded from the filesystem by SkillLoader,
                    // not from individual plugin entries.  Handled below.
                    continue;
                }
                PluginKind::Wasm => self.wasm_tools(plugin).await,
            };

            match result {
                Ok(plugin_tools) => tools.extend(plugin_tools),
                Err(e) => {
                    warn!(
                        plugin_id = %plugin.id,
                        plugin_name = %plugin.name,
                        error = %e,
                        "failed to load tools from plugin, skipping"
                    );
                }
            }
        }

        // Add skill activation tool if skills are available.
        // Also syncs filesystem skills into the plugins table so they
        // appear in the unified plugins UI.
        if let Ok(skill_tools) = self
            .skill_tools(
                user_home,
                workspace_dir,
                workspace_id,
                skill_activation_state,
            )
            .await
        {
            tools.extend(skill_tools);
        }

        Ok(tools)
    }

    /// Returns tool adapters from the MCP pool for a given plugin.
    ///
    /// The MCP server must already be connected in the pool (connections are
    /// managed externally).  Filters tool adapters by the plugin's name to
    /// return only the tools belonging to this specific MCP server.
    async fn mcp_tools(&self, plugin: &Plugin) -> Result<Vec<Arc<dyn Tool>>, PluginError> {
        let pool = self.mcp_pool.lock().await;

        if !pool.is_connected(&plugin.id) {
            return Err(PluginError::Config(format!(
                "MCP server '{}' is not connected in the pool",
                plugin.name,
            )));
        }

        let adapters = pool.tool_adapters();
        let tools: Vec<Arc<dyn Tool>> = adapters
            .into_iter()
            .filter(|a| a.server_name() == plugin.name)
            .map(|a| Arc::new(a) as Arc<dyn Tool>)
            .collect();

        debug!(
            plugin_id = %plugin.id,
            tool_count = tools.len(),
            "loaded MCP tools"
        );

        Ok(tools)
    }

    /// Syncs filesystem skills into the plugins table.
    ///
    /// Scans `user_home` and `workspace_dir` for skill files, registers any
    /// newly-discovered skills in the DB, and returns the names of skills
    /// that are disabled. Call this from agent startup or reload — it is
    /// also called internally by [`tools_for_turn`](Self::tools_for_turn).
    pub async fn sync_filesystem_skills(
        &self,
        user_home: &Path,
        workspace_dir: &Path,
        workspace_id: Option<sober_core::types::WorkspaceId>,
    ) -> Result<Vec<String>, PluginError> {
        use sober_core::types::enums::{PluginOrigin, PluginScope};
        use std::collections::HashMap;

        let catalog = self
            .skill_loader
            .load(user_home, workspace_dir)
            .await
            .map_err(PluginError::Skill)?;

        if catalog.is_empty() {
            return Ok(vec![]);
        }

        // Query all skill plugins (any status).
        let all_skill_filter = PluginFilter {
            kind: Some(PluginKind::Skill),
            ..Default::default()
        };
        let existing_skills = self
            .registry
            .list(all_skill_filter)
            .await
            .unwrap_or_default();

        // Key by (name, workspace_id) so the same skill name can exist
        // in different workspaces without colliding.
        let existing_by_key: HashMap<(&str, Option<sober_core::types::WorkspaceId>), &Plugin> =
            existing_skills
                .iter()
                .map(|p| ((p.name.as_str(), p.workspace_id), p))
                .collect();

        let mut disabled_names: Vec<String> = Vec::new();
        for name in catalog.names() {
            let entry = catalog.get(name).expect("name from catalog");
            let lookup_ws = match entry.source {
                sober_skill::SkillSource::Workspace => workspace_id,
                sober_skill::SkillSource::User => None,
            };

            if let Some(db_plugin) = existing_by_key.get(&(name, lookup_ws)) {
                if db_plugin.status == PluginStatus::Disabled {
                    disabled_names.push(name.to_owned());
                }
            } else {
                // New skill — install through the audit pipeline.
                let (scope, ws_id) = match entry.source {
                    sober_skill::SkillSource::User => (PluginScope::User, None),
                    sober_skill::SkillSource::Workspace => (PluginScope::Workspace, workspace_id),
                };
                let install_req = InstallRequest {
                    name: entry.frontmatter.name.clone(),
                    kind: PluginKind::Skill,
                    version: None,
                    description: Some(entry.frontmatter.description.clone()),
                    origin: PluginOrigin::System,
                    scope,
                    owner_id: None,
                    workspace_id: ws_id,
                    config: serde_json::json!({
                        "path": entry.path.to_string_lossy(),
                        "source": format!("{:?}", entry.source),
                    }),
                    installed_by: None,
                    manifest: None,
                    wasm_bytes: None,
                };
                match self.registry.install(install_req).await {
                    Ok(report) => {
                        debug!(
                            skill_name = name,
                            verdict = ?report.verdict,
                            "skill audit complete"
                        );
                    }
                    Err(PluginError::AlreadyExists(_)) => {
                        // Race condition — another sync registered it first.
                    }
                    Err(e) => {
                        debug!(
                            skill_name = name,
                            error = %e,
                            "failed to register discovered skill"
                        );
                    }
                }
            }
        }

        Ok(disabled_names)
    }

    /// Loads the skill activation tool from the filesystem.
    ///
    /// Calls [`sync_filesystem_skills`](Self::sync_filesystem_skills) to
    /// register new skills and identify disabled ones, then builds the
    /// [`ActivateSkillTool`] excluding disabled skills.
    async fn skill_tools(
        &self,
        user_home: &Path,
        workspace_dir: &Path,
        workspace_id: Option<sober_core::types::WorkspaceId>,
        activation_state: Option<Arc<Mutex<SkillActivationState>>>,
    ) -> Result<Vec<Arc<dyn Tool>>, PluginError> {
        use sober_skill::SkillCatalog;
        use std::collections::HashMap;

        let disabled_names = self
            .sync_filesystem_skills(user_home, workspace_dir, workspace_id)
            .await?;

        let catalog = self
            .skill_loader
            .load(user_home, workspace_dir)
            .await
            .map_err(PluginError::Skill)?;

        if catalog.is_empty() {
            return Ok(vec![]);
        }

        // If no skills are disabled, use the catalog as-is.
        if disabled_names.is_empty() {
            let state = activation_state
                .unwrap_or_else(|| Arc::new(Mutex::new(SkillActivationState::default())));
            return Ok(vec![Arc::new(ActivateSkillTool::new(catalog, state))]);
        }

        // Rebuild the catalog without disabled skills.
        let mut filtered_skills: HashMap<String, sober_skill::SkillEntry> = HashMap::new();
        for name in catalog.names() {
            if !disabled_names.contains(&name.to_owned())
                && let Some(entry) = catalog.get(name)
            {
                filtered_skills.insert(name.to_owned(), entry.clone());
            }
        }

        if filtered_skills.is_empty() {
            return Ok(vec![]);
        }

        let filtered_catalog = Arc::new(SkillCatalog::new(filtered_skills));
        let state = activation_state
            .unwrap_or_else(|| Arc::new(Mutex::new(SkillActivationState::default())));

        Ok(vec![Arc::new(ActivateSkillTool::new(
            filtered_catalog,
            state,
        ))])
    }

    /// Loads tools from a WASM plugin.
    ///
    /// Checks the host cache first; on miss, reads WASM bytes from the path
    /// in the plugin config and creates a new [`PluginHost`].  Returns a
    /// [`PluginTool`] for each tool declared in the manifest.
    async fn wasm_tools(&self, plugin: &Plugin) -> Result<Vec<Arc<dyn Tool>>, PluginError> {
        // Check cache.
        let cached_host = {
            let cache = self.wasm_hosts.read().map_err(|_| {
                PluginError::ExecutionFailed("WASM host cache lock poisoned".into())
            })?;
            cache.get(&plugin.id).cloned()
        };

        let host = match cached_host {
            Some(h) => h,
            None => {
                let wasm_path = plugin
                    .config
                    .get("wasm_path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        PluginError::Config("WASM plugin missing 'wasm_path' in config".into())
                    })?;

                let manifest_toml = plugin
                    .config
                    .get("manifest_toml")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        PluginError::Config("WASM plugin missing 'manifest_toml' in config".into())
                    })?;

                let bytes = tokio::fs::read(wasm_path).await.map_err(|e| {
                    PluginError::ExecutionFailed(format!(
                        "failed to read WASM file at {wasm_path}: {e}"
                    ))
                })?;

                let manifest = PluginManifest::from_toml(manifest_toml)?;
                let new_host = PluginHost::load(&bytes, &manifest)?;
                let host = Arc::new(Mutex::new(new_host));

                let mut cache = self.wasm_hosts.write().map_err(|_| {
                    PluginError::ExecutionFailed("WASM host cache lock poisoned".into())
                })?;
                cache.insert(plugin.id, Arc::clone(&host));

                host
            }
        };

        // Build a PluginTool for each tool declared in the manifest.
        let manifest = {
            let h = host
                .lock()
                .map_err(|_| PluginError::ExecutionFailed("WASM host lock poisoned".into()))?;
            h.manifest().clone()
        };

        let tools: Vec<Arc<dyn Tool>> = manifest
            .tools
            .iter()
            .map(|entry| {
                Arc::new(PluginTool::new(
                    Arc::clone(&host),
                    entry.name.clone(),
                    entry.description.clone(),
                )) as Arc<dyn Tool>
            })
            .collect();

        debug!(
            plugin_id = %plugin.id,
            tool_count = tools.len(),
            "loaded WASM tools"
        );

        Ok(tools)
    }

    /// Evicts a WASM plugin from the host cache.
    ///
    /// The next call to [`tools_for_turn`](Self::tools_for_turn) for this
    /// plugin will reload the WASM bytes and create a fresh [`PluginHost`].
    pub fn evict_wasm_host(&self, plugin_id: &PluginId) {
        if let Ok(mut cache) = self.wasm_hosts.write() {
            cache.remove(plugin_id);
        }
    }

    /// Provides mutable access to the MCP pool for connection management.
    ///
    /// The caller (typically the agent startup code) uses this to connect
    /// MCP servers and run discovery before tools are queried.
    pub async fn mcp_pool(&self) -> tokio::sync::MutexGuard<'_, McpPool> {
        self.mcp_pool.lock().await
    }

    /// Shuts down all MCP server connections.
    pub async fn shutdown(&self) {
        let mut pool = self.mcp_pool.lock().await;
        pool.shutdown().await;
    }

    /// Returns a reference to the plugin repository.
    pub fn repo(&self) -> &R {
        self.registry.repo()
    }

    /// Returns a reference to the skill loader.
    pub fn skill_loader(&self) -> &Arc<SkillLoader> {
        &self.skill_loader
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sober_core::error::AppError;
    use sober_core::types::domain::Plugin;
    use sober_core::types::enums::{PluginKind, PluginOrigin, PluginScope, PluginStatus};
    use sober_core::types::ids::{PluginId, UserId};
    use sober_core::types::input::{CreatePlugin, CreatePluginAuditLog, PluginFilter};
    use sober_core::types::repo::PluginRepo;
    use sober_mcp::McpConfig;
    use std::time::Duration;

    // -----------------------------------------------------------------------
    // Mock PluginRepo
    // -----------------------------------------------------------------------

    struct MockPluginRepo {
        plugins: tokio::sync::Mutex<Vec<Plugin>>,
    }

    impl MockPluginRepo {
        fn new(plugins: Vec<Plugin>) -> Self {
            Self {
                plugins: tokio::sync::Mutex::new(plugins),
            }
        }
    }

    impl PluginRepo for MockPluginRepo {
        fn create(
            &self,
            _input: CreatePlugin,
        ) -> impl std::future::Future<Output = Result<Plugin, AppError>> + Send {
            async { Err(AppError::Internal("not implemented".into())) }
        }

        fn get_by_id(
            &self,
            id: PluginId,
        ) -> impl std::future::Future<Output = Result<Plugin, AppError>> + Send {
            async move {
                let plugins = self.plugins.lock().await;
                plugins
                    .iter()
                    .find(|p| p.id == id)
                    .cloned()
                    .ok_or_else(|| AppError::NotFound(format!("plugin {id}")))
            }
        }

        fn get_by_name(
            &self,
            name: &str,
        ) -> impl std::future::Future<Output = Result<Plugin, AppError>> + Send {
            let name = name.to_owned();
            async move {
                let plugins = self.plugins.lock().await;
                plugins
                    .iter()
                    .find(|p| p.name == name)
                    .cloned()
                    .ok_or_else(|| AppError::NotFound(name))
            }
        }

        fn list(
            &self,
            filter: PluginFilter,
        ) -> impl std::future::Future<Output = Result<Vec<Plugin>, AppError>> + Send {
            async move {
                let plugins = self.plugins.lock().await;
                let filtered = plugins
                    .iter()
                    .filter(|p| {
                        filter.status.map_or(true, |s| p.status == s)
                            && filter.owner_id.map_or(true, |o| p.owner_id == Some(o))
                            && filter.kind.map_or(true, |k| p.kind == k)
                    })
                    .cloned()
                    .collect();
                Ok(filtered)
            }
        }

        fn update_status(
            &self,
            _id: PluginId,
            _status: PluginStatus,
        ) -> impl std::future::Future<Output = Result<(), AppError>> + Send {
            async { Ok(()) }
        }

        fn update_config(
            &self,
            _id: PluginId,
            _config: serde_json::Value,
        ) -> impl std::future::Future<Output = Result<(), AppError>> + Send {
            async { Ok(()) }
        }

        fn delete(
            &self,
            _id: PluginId,
        ) -> impl std::future::Future<Output = Result<(), AppError>> + Send {
            async { Ok(()) }
        }

        fn create_audit_log(
            &self,
            _input: CreatePluginAuditLog,
        ) -> impl std::future::Future<
            Output = Result<sober_core::types::domain::PluginAuditLog, AppError>,
        > + Send {
            async { Err(AppError::Internal("not implemented".into())) }
        }

        fn list_audit_logs(
            &self,
            _plugin_id: PluginId,
            _limit: i64,
        ) -> impl std::future::Future<
            Output = Result<Vec<sober_core::types::domain::PluginAuditLog>, AppError>,
        > + Send {
            async { Ok(vec![]) }
        }

        fn get_kv_data(
            &self,
            _plugin_id: PluginId,
            _key: &str,
        ) -> impl std::future::Future<Output = Result<Option<serde_json::Value>, AppError>> + Send
        {
            async { Ok(None) }
        }

        fn set_kv_data(
            &self,
            _plugin_id: PluginId,
            _key: &str,
            _value: serde_json::Value,
        ) -> impl std::future::Future<Output = Result<(), AppError>> + Send {
            async { Ok(()) }
        }

        fn update_scope(
            &self,
            _id: PluginId,
            _scope: sober_core::types::PluginScope,
        ) -> impl std::future::Future<Output = Result<(), AppError>> + Send {
            async { Ok(()) }
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_plugin(kind: PluginKind, name: &str, config: serde_json::Value) -> Plugin {
        Plugin {
            id: PluginId::new(),
            name: name.to_owned(),
            kind,
            version: Some("0.1.0".to_owned()),
            description: None,
            origin: PluginOrigin::User,
            scope: PluginScope::User,
            owner_id: Some(UserId::new()),
            workspace_id: None,
            status: PluginStatus::Enabled,
            config,
            installed_by: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn make_manager(plugins: Vec<Plugin>) -> PluginManager<MockPluginRepo> {
        let repo = MockPluginRepo::new(plugins);
        let pool = McpPool::new(McpConfig::default());
        let loader = Arc::new(SkillLoader::new(Duration::from_secs(300)));
        PluginManager::new(repo, pool, loader)
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn tools_for_turn_returns_empty_when_no_plugins() {
        let manager = make_manager(vec![]);
        let tools = manager
            .tools_for_turn(
                UserId::new(),
                Path::new("/nonexistent-home"),
                Path::new("/nonexistent"),
                None,
                None,
            )
            .await
            .expect("should succeed");

        // No plugins and no skills at /nonexistent => empty.
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn skill_plugins_are_skipped_in_loop() {
        // A Skill-kind plugin in the DB should not produce tools via
        // the per-plugin loop — skills come from SkillLoader instead.
        let skill_plugin = make_plugin(PluginKind::Skill, "test-skill", serde_json::json!({}));
        let user_id = skill_plugin.owner_id.expect("owner set");
        let manager = make_manager(vec![skill_plugin]);

        let tools = manager
            .tools_for_turn(
                user_id,
                Path::new("/nonexistent-home"),
                Path::new("/nonexistent"),
                None,
                None,
            )
            .await
            .expect("should succeed");

        // No actual skill directories at /nonexistent, so empty.
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn wasm_plugin_missing_config_is_skipped() {
        // A WASM plugin with incomplete config should log a warning and
        // be skipped rather than failing the entire turn.
        let wasm_plugin = make_plugin(
            PluginKind::Wasm,
            "bad-wasm",
            serde_json::json!({}), // Missing wasm_path and manifest_toml
        );
        let user_id = wasm_plugin.owner_id.expect("owner set");
        let manager = make_manager(vec![wasm_plugin]);

        let tools = manager
            .tools_for_turn(
                user_id,
                Path::new("/nonexistent-home"),
                Path::new("/nonexistent"),
                None,
                None,
            )
            .await
            .expect("should succeed despite bad plugin config");

        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn mcp_plugin_not_connected_is_skipped() {
        // An MCP plugin whose server is not connected in the pool should
        // be skipped gracefully.
        let mcp_plugin = make_plugin(
            PluginKind::Mcp,
            "not-connected",
            serde_json::json!({"command": "npx", "args": ["-y", "some-server"]}),
        );
        let user_id = mcp_plugin.owner_id.expect("owner set");
        let manager = make_manager(vec![mcp_plugin]);

        let tools = manager
            .tools_for_turn(
                user_id,
                Path::new("/nonexistent-home"),
                Path::new("/nonexistent"),
                None,
                None,
            )
            .await
            .expect("should succeed despite unconnected MCP server");

        assert!(tools.is_empty());
    }

    #[test]
    fn evict_wasm_host_removes_from_cache() {
        let manager = make_manager(vec![]);
        let id = PluginId::new();

        // Eviction on a missing key is a no-op.
        manager.evict_wasm_host(&id);

        let cache = manager.wasm_hosts.read().expect("lock");
        assert!(cache.is_empty());
    }

    #[tokio::test]
    async fn shutdown_is_safe_on_empty_pool() {
        let manager = make_manager(vec![]);
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn mcp_pool_accessor_returns_guard() {
        let manager = make_manager(vec![]);
        let pool = manager.mcp_pool().await;
        assert_eq!(pool.server_count(), 0);
    }

    #[test]
    fn repo_accessor_returns_repo() {
        let manager = make_manager(vec![]);
        let _repo = manager.repo();
    }

    #[test]
    fn skill_loader_accessor_returns_loader() {
        let manager = make_manager(vec![]);
        let loader = manager.skill_loader();
        assert!(Arc::strong_count(loader) >= 1);
    }

    #[tokio::test]
    async fn disabled_plugins_are_filtered_out() {
        // Create a plugin that's disabled — it should not appear in results.
        let mut disabled = make_plugin(PluginKind::Wasm, "disabled", serde_json::json!({}));
        disabled.status = PluginStatus::Disabled;
        let user_id = disabled.owner_id.expect("owner set");
        let manager = make_manager(vec![disabled]);

        let tools = manager
            .tools_for_turn(
                user_id,
                Path::new("/nonexistent-home"),
                Path::new("/nonexistent"),
                None,
                None,
            )
            .await
            .expect("should succeed");

        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn other_users_plugins_are_not_returned() {
        let plugin = make_plugin(
            PluginKind::Wasm,
            "other-user-plugin",
            serde_json::json!({"wasm_path": "/tmp/test.wasm", "manifest_toml": "x"}),
        );
        // Query with a different user ID.
        let different_user = UserId::new();
        let manager = make_manager(vec![plugin]);

        let tools = manager
            .tools_for_turn(
                different_user,
                Path::new("/nonexistent-home"),
                Path::new("/nonexistent"),
                None,
                None,
            )
            .await
            .expect("should succeed");

        assert!(tools.is_empty());
    }
}
