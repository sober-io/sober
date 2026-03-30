//! Agent tool for generating plugins via LLM.
//!
//! Exposes a `/generate-plugin` tool that delegates to [`PluginGenerator`] from
//! `sober-plugin-gen` to create WASM plugins or markdown skills from a
//! natural-language description.

use std::path::PathBuf;
use std::sync::Arc;

use sober_core::types::enums::{
    ArtifactKind, ArtifactRelation, ArtifactState, PluginKind, PluginOrigin, PluginScope,
};
use sober_core::types::ids::{ConversationId, UserId, WorkspaceId};
use sober_core::types::input::{ArtifactFilter, CreateArtifact, PluginFilter};
use sober_core::types::repo::{ArtifactRepo, PluginRepo};
use sober_core::types::tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
use sober_plugin::audit::AuditRequest;
use sober_plugin::registry::InstallRequest;
use sober_plugin::{AuditPipeline, PluginManager};
use sober_plugin_gen::PluginGenerator;
use sober_workspace::BlobStore;

/// Configuration for [`GeneratePluginTool`] construction.
///
/// Groups the dependencies and context that the tool needs beyond the
/// plugin manager (which is always required).
pub struct GeneratePluginConfig<A: ArtifactRepo> {
    pub generator: Arc<PluginGenerator>,
    pub blob_store: Arc<BlobStore>,
    pub artifact_repo: Option<Arc<A>>,
    pub workspace_dir: Option<PathBuf>,
    pub workspace_id: Option<WorkspaceId>,
    pub conversation_id: Option<ConversationId>,
    pub user_id: UserId,
    /// Built-in tool names that generated plugins must not shadow.
    pub reserved_tool_names: Vec<String>,
}

/// Built-in tool that generates plugins (WASM or skill) via LLM.
///
/// Delegates to [`PluginGenerator`] for the actual generation work, saves the
/// output files to the workspace, and registers the plugin in the database
/// via the [`PluginManager`]'s repository.
///
/// Generated WASM binaries are stored content-addressed in [`BlobStore`] and
/// tracked as workspace [`Artifact`](sober_core::types::domain::Artifact)
/// records when an artifact repository and workspace context are available.
pub struct GeneratePluginTool<R: PluginRepo, A: ArtifactRepo> {
    generator: Arc<PluginGenerator>,
    plugin_manager: Arc<PluginManager<R>>,
    /// Content-addressed blob store for persisting generated WASM binaries.
    blob_store: Arc<BlobStore>,
    /// Artifact repository for tracking generated WASM binaries as artifacts.
    /// `None` when no workspace is active (user-scoped plugins).
    artifact_repo: Option<Arc<A>>,
    workspace_dir: Option<PathBuf>,
    workspace_id: Option<WorkspaceId>,
    conversation_id: Option<ConversationId>,
    user_id: UserId,
    reserved_tool_names: Vec<String>,
}

impl<R: PluginRepo, A: ArtifactRepo> GeneratePluginTool<R, A> {
    /// Creates a new generate-plugin tool.
    ///
    /// `config.workspace_dir` is the directory where generated plugin source
    /// files will be saved for human inspection. If `None`, the tool returns a
    /// message asking the user to set up a workspace first.
    ///
    /// Generated WASM bytes are stored in `config.blob_store` content-addressed
    /// rather than written to the filesystem, so they survive workspace moves.
    /// When `config.artifact_repo` and `config.workspace_id` are both provided
    /// the blob is also registered as a workspace artifact.
    pub fn new(plugin_manager: Arc<PluginManager<R>>, config: GeneratePluginConfig<A>) -> Self {
        Self {
            generator: config.generator,
            plugin_manager,
            blob_store: config.blob_store,
            artifact_repo: config.artifact_repo,
            workspace_dir: config.workspace_dir,
            workspace_id: config.workspace_id,
            conversation_id: config.conversation_id,
            user_id: config.user_id,
            reserved_tool_names: config.reserved_tool_names,
        }
    }
}

impl<R: PluginRepo + 'static, A: ArtifactRepo + 'static> Tool for GeneratePluginTool<R, A> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "generate_plugin".to_owned(),
            description:
                "Generate a new plugin (WASM or skill) from a natural-language description. \
                The plugin is generated via LLM, saved to the workspace, and registered."
                    .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Plugin name (lowercase, no spaces, e.g. 'weather-lookup')."
                    },
                    "description": {
                        "type": "string",
                        "description": "What the plugin should do — a natural-language description."
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["wasm", "skill"],
                        "description": "Plugin type to generate (default: 'skill')."
                    },
                    "capabilities": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Required capabilities for WASM plugins: key_value, network, secret_read, tool_call, memory_read, memory_write, conversation_read, schedule, metrics, filesystem."
                    }
                },
                "required": ["name", "description"]
            }),
            context_modifying: true,
            internal: false,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

impl<R: PluginRepo + 'static, A: ArtifactRepo + 'static> GeneratePluginTool<R, A> {
    /// Inner async implementation for [`Tool::execute`].
    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'name'".into()))?;

        if name.is_empty() {
            return Err(ToolError::InvalidInput("name must not be empty".into()));
        }

        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidInput("missing required field 'description'".into())
            })?;

        if description.is_empty() {
            return Err(ToolError::InvalidInput(
                "description must not be empty".into(),
            ));
        }

        let kind = input
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("skill");

        let result = match kind {
            "wasm" => self.generate_wasm(name, description, &input).await,
            "skill" => self.generate_skill(name, description).await,
            other => Err(ToolError::InvalidInput(format!(
                "unknown plugin kind '{other}', expected 'wasm' or 'skill'"
            ))),
        };

        // Convert execution errors into is_error outputs so the LLM
        // reports the failure to the user instead of trying alternatives.
        match result {
            Ok(output) => Ok(output),
            Err(ToolError::InvalidInput(msg)) => Err(ToolError::InvalidInput(msg)),
            Err(e) => Ok(ToolOutput {
                content: format!("Plugin generation failed: {e}"),
                is_error: true,
            }),
        }
    }

    /// Registers a plugin through the audit pipeline and persists it.
    async fn register_plugin(
        &self,
        name: &str,
        description: &str,
        kind: PluginKind,
        config: serde_json::Value,
        manifest: Option<sober_plugin::PluginManifest>,
        wasm_bytes: Option<Vec<u8>>,
    ) -> Result<(), ToolError> {
        let scope = if self.workspace_id.is_some() {
            PluginScope::Workspace
        } else {
            PluginScope::User
        };

        let install_req = InstallRequest {
            name: name.to_owned(),
            kind,
            version: Some("0.1.0".to_owned()),
            description: Some(description.to_owned()),
            origin: PluginOrigin::Agent,
            scope,
            owner_id: Some(self.user_id),
            workspace_id: self.workspace_id,
            config,
            installed_by: Some(self.user_id),
            manifest,
            wasm_bytes,
            reserved_tool_names: self.reserved_tool_names.clone(),
        };

        let report = self
            .plugin_manager
            .registry()
            .install(install_req)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to register plugin: {e}")))?;

        if !report.is_approved() {
            return Err(ToolError::ExecutionFailed(format!(
                "plugin rejected by audit: {}",
                report.rejection_reason().unwrap_or("unknown")
            )));
        }

        Ok(())
    }

    /// Generates a WASM plugin, stores it content-addressed, and registers it.
    ///
    /// If a plugin with the same name already exists for this user/workspace
    /// (a regeneration), the existing plugin's config is updated to reference
    /// the new blob, the old artifact is archived, a new artifact is created
    /// that supersedes it, and the stale WASM host is evicted from cache.
    ///
    /// For first-time generation the plugin is installed through the normal
    /// audit pipeline.
    ///
    /// The WASM binary is stored in [`BlobStore`] (content-addressed by
    /// SHA-256). Source and manifest files are written to the workspace for
    /// human inspection, but the WASM binary itself lives only in the blob
    /// store.
    async fn generate_wasm(
        &self,
        name: &str,
        description: &str,
        input: &serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        let workspace_dir = self.workspace_dir.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "No workspace directory configured. Create a workspace first.".into(),
            )
        })?;

        let capabilities: Vec<String> = input
            .get("capabilities")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let generated = self
            .generator
            .generate_wasm(name, description, &capabilities)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("WASM generation failed: {e}")))?;

        // Store WASM bytes content-addressed in BlobStore.
        // The binary lives only here — no filesystem copy of the .wasm file.
        let wasm_blob_key = self
            .blob_store
            .store(&generated.wasm_bytes)
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("failed to store WASM in blob store: {e}"))
            })?;

        // Store manifest as a blob too — kept inline in config for fast loading,
        // but also content-addressed for future import/export packaging.
        let manifest_blob_key = self
            .blob_store
            .store(generated.manifest.as_bytes())
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("failed to store manifest in blob store: {e}"))
            })?;

        // Save source and manifest to .sober/plugins/<name>/ for human inspection.
        let plugin_dir = workspace_dir.join(".sober").join("plugins").join(name);
        tokio::fs::create_dir_all(&plugin_dir)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to create plugin dir: {e}")))?;

        tokio::fs::write(plugin_dir.join("plugin.toml"), &generated.manifest)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to write manifest: {e}")))?;

        tokio::fs::write(plugin_dir.join("src_lib.rs"), &generated.source)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to write source: {e}")))?;

        // Also save the Cargo.toml used for compilation so the workspace copy
        // accurately reflects the crates.io dependency (not a local path).
        let cargo_toml_content = sober_plugin_gen::cargo_toml(name);
        tokio::fs::write(plugin_dir.join("Cargo.toml"), cargo_toml_content.as_bytes())
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to write Cargo.toml: {e}")))?;

        // Parse the manifest for the audit pipeline.
        let manifest = sober_plugin::PluginManifest::from_toml(&generated.manifest)
            .map_err(|e| ToolError::ExecutionFailed(format!("invalid manifest: {e}")))?;

        let new_config = serde_json::json!({
            "wasm_blob_key": wasm_blob_key,
            "manifest_blob_key": manifest_blob_key,
            "source_path": plugin_dir.join("src_lib.rs").to_string_lossy(),
            "manifest_toml": generated.manifest,
        });

        // Check if a plugin with this name already exists for this user/workspace
        // to distinguish regeneration from first-time creation.
        let scope = if self.workspace_id.is_some() {
            PluginScope::Workspace
        } else {
            PluginScope::User
        };

        let existing_plugins = self
            .plugin_manager
            .registry()
            .repo()
            .list(PluginFilter {
                name: Some(name.to_owned()),
                scope: Some(scope),
                owner_id: Some(self.user_id),
                workspace_id: self.workspace_id,
                ..Default::default()
            })
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("failed to query existing plugin: {e}"))
            })?;

        let is_regeneration = !existing_plugins.is_empty();

        if is_regeneration {
            let existing = &existing_plugins[0];
            let plugin_id = existing.id;

            // Audit the new WASM bytes before committing any changes.
            // This ensures regenerated plugins go through the same security
            // pipeline as first-time installations.
            let audit_request = AuditRequest {
                name: name.to_owned(),
                kind: PluginKind::Wasm,
                origin: PluginOrigin::Agent,
                config: new_config.clone(),
                manifest: Some(manifest.clone()),
                wasm_bytes: Some(generated.wasm_bytes.clone()),
                reserved_tool_names: self.reserved_tool_names.clone(),
            };
            let report = AuditPipeline::audit(&audit_request);
            if !report.is_approved() {
                return Ok(ToolOutput {
                    content: format!(
                        "Plugin regeneration failed: rejected by audit — {}",
                        report.rejection_reason().unwrap_or("unknown")
                    ),
                    is_error: true,
                });
            }

            // Evict the stale WASM host BEFORE updating config.  If config
            // update fails, the next load simply re-reads the old blob (still
            // valid).  The reverse order risks serving stale cached bytes
            // while config already points to the new blob.
            self.plugin_manager.evict_wasm_host(&plugin_id);

            // Update the plugin's config to reference the new blob.
            self.plugin_manager
                .registry()
                .repo()
                .update_config(plugin_id, new_config)
                .await
                .map_err(|e| {
                    ToolError::ExecutionFailed(format!("failed to update plugin config: {e}"))
                })?;

            tracing::info!(
                plugin_id = %plugin_id,
                plugin_name = name,
                blob_key = %wasm_blob_key,
                "regenerated WASM plugin — audit passed, config updated, WASM host evicted"
            );

            // Archive old artifact and create a new one when a workspace is present.
            if let (Some(ws_id), Some(artifact_repo)) =
                (self.workspace_id, self.artifact_repo.as_ref())
            {
                let expected_title = format!("plugin:{name}");

                // Find the existing non-archived artifact for this plugin.
                let all_artifacts = artifact_repo
                    .list_by_workspace(
                        ws_id,
                        ArtifactFilter {
                            kind: Some(ArtifactKind::Document),
                            ..Default::default()
                        },
                    )
                    .await
                    .unwrap_or_default();

                let old_artifact = all_artifacts
                    .into_iter()
                    .find(|a| a.title == expected_title && a.state != ArtifactState::Archived);

                let old_artifact_id = old_artifact.as_ref().map(|a| a.id);

                // Archive the old artifact.
                if let Some(old_id) = old_artifact_id
                    && let Err(e) = artifact_repo
                        .update_state(old_id, ArtifactState::Archived)
                        .await
                {
                    tracing::error!(
                        plugin_name = name,
                        artifact_id = %old_id,
                        error = %e,
                        "failed to archive old artifact for regenerated WASM plugin"
                    );
                }

                // Create a new artifact for the updated blob.
                let create_input = CreateArtifact {
                    workspace_id: ws_id,
                    user_id: self.user_id,
                    kind: ArtifactKind::Document,
                    title: expected_title,
                    description: Some(description.to_owned()),
                    storage_type: "blob".to_owned(),
                    blob_key: Some(wasm_blob_key.clone()),
                    created_by: None, // None = agent-created
                    conversation_id: self.conversation_id,
                    parent_id: old_artifact_id,
                    ..Default::default()
                };

                match artifact_repo.create(create_input).await {
                    Ok(new_artifact) => {
                        // Link the new artifact as superseding the old one.
                        if let Some(old_id) = old_artifact_id
                            && let Err(e) = artifact_repo
                                .add_relation(new_artifact.id, old_id, ArtifactRelation::Supersedes)
                                .await
                        {
                            tracing::error!(
                                plugin_name = name,
                                error = %e,
                                "failed to add supersedes relation between artifacts"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            plugin_name = name,
                            blob_key = %wasm_blob_key,
                            error = %e,
                            "failed to create artifact record for regenerated WASM plugin"
                        );
                    }
                }
            }

            Ok(ToolOutput {
                content: format!(
                    "Regenerated WASM plugin '{name}' (updated existing plugin).\n\
                     Source saved to: {}\n\
                     WASM stored as new blob: {wasm_blob_key}\n\
                     Capabilities: {}\n\
                     Status: enabled (previous version archived)",
                    plugin_dir.display(),
                    if capabilities.is_empty() {
                        "none".to_owned()
                    } else {
                        capabilities.join(", ")
                    },
                ),
                is_error: false,
            })
        } else {
            // First-time creation — register through the audit pipeline.
            self.register_plugin(
                name,
                description,
                PluginKind::Wasm,
                new_config,
                Some(manifest),
                Some(generated.wasm_bytes),
            )
            .await?;

            // Create an artifact record when a workspace context is available.
            if let (Some(ws_id), Some(artifact_repo)) =
                (self.workspace_id, self.artifact_repo.as_ref())
            {
                let create_input = CreateArtifact {
                    workspace_id: ws_id,
                    user_id: self.user_id,
                    kind: ArtifactKind::Document,
                    title: format!("plugin:{name}"),
                    description: Some(description.to_owned()),
                    storage_type: "blob".to_owned(),
                    blob_key: Some(wasm_blob_key.clone()),
                    created_by: None, // None = agent-created
                    conversation_id: self.conversation_id,
                    ..Default::default()
                };

                // Non-fatal: log an error if artifact creation fails, but don't
                // fail the whole tool invocation — the plugin is already registered.
                if let Err(e) = artifact_repo.create(create_input).await {
                    tracing::error!(
                        plugin_name = name,
                        blob_key = %wasm_blob_key,
                        error = %e,
                        "failed to create artifact record for generated WASM plugin"
                    );
                }
            }

            Ok(ToolOutput {
                content: format!(
                    "Generated WASM plugin '{name}'.\n\
                     Source saved to: {}\n\
                     WASM stored as blob: {wasm_blob_key}\n\
                     Capabilities: {}\n\
                     Status: enabled (audit passed)",
                    plugin_dir.display(),
                    if capabilities.is_empty() {
                        "none".to_owned()
                    } else {
                        capabilities.join(", ")
                    },
                ),
                is_error: false,
            })
        }
    }

    /// Generates a skill and saves it to the workspace.
    async fn generate_skill(&self, name: &str, description: &str) -> Result<ToolOutput, ToolError> {
        let workspace_dir = self.workspace_dir.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "No workspace directory configured. Create a workspace first.".into(),
            )
        })?;

        let skill_content = self
            .generator
            .generate_skill(name, description)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("skill generation failed: {e}")))?;

        // Save skill markdown to .sober/skills/<name>/SKILL.md
        // SkillLoader expects this directory structure for discovery.
        let skill_dir = workspace_dir.join(".sober").join("skills").join(name);
        tokio::fs::create_dir_all(&skill_dir)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to create skill dir: {e}")))?;

        let skill_path = skill_dir.join("SKILL.md");
        tokio::fs::write(&skill_path, &skill_content)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to write skill file: {e}")))?;

        // Register the skill through the audit pipeline.
        self.register_plugin(
            name,
            description,
            PluginKind::Skill,
            serde_json::json!({
                "path": skill_path.to_string_lossy(),
            }),
            None,
            None,
        )
        .await?;

        Ok(ToolOutput {
            content: format!(
                "Generated skill '{name}'.\n\
                 Saved to: {}\n\
                 Status: enabled (audit passed)\n\n\
                 The skill will be available in the next conversation turn.",
                skill_path.display(),
            ),
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::pin::Pin;
    use std::sync::Mutex;
    use std::time::Duration;

    use async_trait::async_trait;
    use futures::Stream;
    use sober_core::error::AppError;
    use sober_core::types::domain::{Artifact, Plugin, PluginAuditLog};
    use sober_core::types::enums::{ArtifactRelation, ArtifactState, PluginStatus};
    use sober_core::types::ids::{ArtifactId, PluginId};
    use sober_core::types::input::{
        ArtifactFilter, CreateArtifact, CreatePlugin, CreatePluginAuditLog, PluginFilter,
    };
    use sober_llm::{
        CompletionRequest, CompletionResponse, EngineCapabilities, LlmEngine, LlmError, Message,
        types::StreamChunk,
    };
    use sober_mcp::{McpConfig, McpPool};
    use sober_skill::SkillLoader;

    // -----------------------------------------------------------------------
    // Mock PluginRepo
    // -----------------------------------------------------------------------

    struct MockPluginRepo {
        created: Mutex<Vec<CreatePlugin>>,
        /// Pre-seeded plugins returned by `list()` (simulates existing records).
        existing: Mutex<Vec<Plugin>>,
        /// Tracks `update_config` calls: (plugin_id, config).
        config_updates: Mutex<Vec<(PluginId, serde_json::Value)>>,
    }

    impl MockPluginRepo {
        fn new() -> Self {
            Self {
                created: Mutex::new(vec![]),
                existing: Mutex::new(vec![]),
                config_updates: Mutex::new(vec![]),
            }
        }

        /// Pre-seeds the mock with a plugin that will be returned by `list()`.
        fn with_existing(self, plugin: Plugin) -> Self {
            self.existing.lock().expect("lock").push(plugin);
            self
        }

        fn created_count(&self) -> usize {
            self.created.lock().expect("lock").len()
        }

        fn last_created(&self) -> Option<CreatePlugin> {
            self.created.lock().expect("lock").last().cloned()
        }

        fn config_update_count(&self) -> usize {
            self.config_updates.lock().expect("lock").len()
        }

        fn last_config_update(&self) -> Option<(PluginId, serde_json::Value)> {
            self.config_updates.lock().expect("lock").last().cloned()
        }
    }

    impl PluginRepo for MockPluginRepo {
        fn create(
            &self,
            input: CreatePlugin,
        ) -> impl std::future::Future<Output = Result<Plugin, AppError>> + Send {
            self.created.lock().expect("lock").push(input.clone());
            async move {
                Ok(Plugin {
                    id: PluginId::new(),
                    name: input.name,
                    kind: input.kind,
                    version: input.version,
                    description: input.description,
                    origin: input.origin,
                    scope: input.scope,
                    owner_id: input.owner_id,
                    workspace_id: input.workspace_id,
                    status: input.status,
                    config: input.config,
                    installed_by: None,
                    installed_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                })
            }
        }

        fn get_by_id(
            &self,
            _id: PluginId,
        ) -> impl std::future::Future<Output = Result<Plugin, AppError>> + Send {
            async { Err(AppError::Internal("not implemented".into())) }
        }

        fn get_by_name(
            &self,
            _name: &str,
        ) -> impl std::future::Future<Output = Result<Plugin, AppError>> + Send {
            async { Err(AppError::Internal("not implemented".into())) }
        }

        fn list(
            &self,
            filter: PluginFilter,
        ) -> impl std::future::Future<Output = Result<Vec<Plugin>, AppError>> + Send {
            // Return pre-seeded plugins that match the filter (name, scope, owner_id).
            let existing = self.existing.lock().expect("lock").clone();
            let matched: Vec<Plugin> = existing
                .into_iter()
                .filter(|p| {
                    filter.name.as_deref().map_or(true, |n| p.name == n)
                        && filter.scope.map_or(true, |s| p.scope == s)
                        && filter.owner_id.map_or(true, |o| p.owner_id == Some(o))
                        && filter
                            .workspace_id
                            .map_or(true, |w| p.workspace_id == Some(w))
                        && filter.status.map_or(true, |s| p.status == s)
                })
                .collect();
            async move { Ok(matched) }
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
            id: PluginId,
            config: serde_json::Value,
        ) -> impl std::future::Future<Output = Result<(), AppError>> + Send {
            self.config_updates.lock().expect("lock").push((id, config));
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
            input: CreatePluginAuditLog,
        ) -> impl std::future::Future<Output = Result<PluginAuditLog, AppError>> + Send {
            async move {
                Ok(PluginAuditLog {
                    id: uuid::Uuid::now_v7(),
                    plugin_id: input.plugin_id,
                    plugin_name: input.plugin_name,
                    kind: input.kind,
                    origin: input.origin,
                    stages: input.stages,
                    verdict: input.verdict,
                    rejection_reason: input.rejection_reason,
                    audited_at: chrono::Utc::now(),
                    audited_by: input.audited_by,
                })
            }
        }

        fn list_audit_logs(
            &self,
            _plugin_id: PluginId,
            _limit: i64,
        ) -> impl std::future::Future<Output = Result<Vec<PluginAuditLog>, AppError>> + Send
        {
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

        async fn delete_kv_data(&self, _plugin_id: PluginId, _key: &str) -> Result<(), AppError> {
            Ok(())
        }

        async fn list_kv_keys(
            &self,
            _plugin_id: PluginId,
            _prefix: Option<&str>,
        ) -> Result<Vec<String>, AppError> {
            Ok(vec![])
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
    // Mock ArtifactRepo
    // -----------------------------------------------------------------------

    struct MockArtifactRepo;

    impl ArtifactRepo for MockArtifactRepo {
        fn create(
            &self,
            input: CreateArtifact,
        ) -> impl std::future::Future<Output = Result<Artifact, AppError>> + Send {
            async move {
                Ok(Artifact {
                    id: ArtifactId::new(),
                    workspace_id: input.workspace_id,
                    user_id: input.user_id,
                    kind: input.kind,
                    state: ArtifactState::Draft,
                    title: input.title,
                    description: input.description,
                    storage_type: input.storage_type,
                    git_repo: input.git_repo,
                    git_ref: input.git_ref,
                    blob_key: input.blob_key,
                    inline_content: input.inline_content,
                    created_by: input.created_by,
                    conversation_id: input.conversation_id,
                    task_id: input.task_id,
                    parent_id: input.parent_id,
                    reviewed_by: None,
                    reviewed_at: None,
                    metadata: serde_json::Value::Null,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                })
            }
        }

        fn get_by_id(
            &self,
            _id: ArtifactId,
        ) -> impl std::future::Future<Output = Result<Artifact, AppError>> + Send {
            async { Err(AppError::Internal("not implemented".into())) }
        }

        fn list_by_workspace(
            &self,
            _workspace_id: WorkspaceId,
            _filter: ArtifactFilter,
        ) -> impl std::future::Future<Output = Result<Vec<Artifact>, AppError>> + Send {
            async { Ok(vec![]) }
        }

        fn list_visible(
            &self,
            _workspace_id: WorkspaceId,
            _is_admin: bool,
        ) -> impl std::future::Future<Output = Result<Vec<Artifact>, AppError>> + Send {
            async { Ok(vec![]) }
        }

        fn update_state(
            &self,
            _id: ArtifactId,
            _state: ArtifactState,
        ) -> impl std::future::Future<Output = Result<(), AppError>> + Send {
            async { Ok(()) }
        }

        fn add_relation(
            &self,
            _source: ArtifactId,
            _target: ArtifactId,
            _relation: ArtifactRelation,
        ) -> impl std::future::Future<Output = Result<(), AppError>> + Send {
            async { Ok(()) }
        }
    }

    // -----------------------------------------------------------------------
    // Mock LLM
    // -----------------------------------------------------------------------

    struct MockLlm {
        response: String,
    }

    impl MockLlm {
        fn new(response: &str) -> Arc<Self> {
            Arc::new(Self {
                response: response.to_owned(),
            })
        }
    }

    #[async_trait]
    impl LlmEngine for MockLlm {
        async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
            Ok(CompletionResponse {
                id: "mock-id".to_string(),
                choices: vec![sober_llm::types::Choice {
                    index: 0,
                    message: Message::assistant(&self.response),
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
            })
        }

        async fn stream(
            &self,
            _req: CompletionRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send>>, LlmError>
        {
            Err(LlmError::Unsupported("mock".to_string()))
        }

        async fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, LlmError> {
            Err(LlmError::Unsupported("mock".to_string()))
        }

        fn capabilities(&self) -> EngineCapabilities {
            EngineCapabilities {
                supports_tools: false,
                supports_streaming: false,
                supports_embeddings: false,
                max_context_tokens: 8192,
            }
        }

        fn model_id(&self) -> &str {
            "mock/model"
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_manager(repo: MockPluginRepo) -> Arc<PluginManager<MockPluginRepo>> {
        let pool = McpPool::new(McpConfig::default());
        let loader = Arc::new(SkillLoader::new(Duration::from_secs(300)));
        Arc::new(PluginManager::new(repo, pool, loader, None))
    }

    /// Creates a tool with a temporary blob store directory.
    ///
    /// Returns the tool and the `TempDir` (as `blob_dir`) so the caller can
    /// keep it alive for the duration of the test — dropping it deletes the
    /// temporary directory and invalidates the blob store.
    fn make_tool(
        workspace_dir: Option<PathBuf>,
    ) -> (
        GeneratePluginTool<MockPluginRepo, MockArtifactRepo>,
        tempfile::TempDir,
    ) {
        let llm = MockLlm::new(
            "---\nname: test\ndescription: A test skill.\n---\n\n## Instructions\nDo the thing.",
        );
        let generator = Arc::new(PluginGenerator::new(llm, "test-model"));
        let manager = make_manager(MockPluginRepo::new());
        // Kept alive so the blob store's backing directory survives the test.
        let blob_dir = tempfile::tempdir().expect("blob tempdir");
        let blob_store = Arc::new(BlobStore::new(blob_dir.path().to_path_buf()));
        let tool = GeneratePluginTool::new(
            manager,
            GeneratePluginConfig {
                generator,
                blob_store,
                artifact_repo: None::<Arc<MockArtifactRepo>>,
                workspace_dir,
                workspace_id: None,
                conversation_id: None,
                user_id: UserId::new(),
                reserved_tool_names: vec![],
            },
        );
        (tool, blob_dir)
    }

    fn make_tool_with_manager(
        workspace_dir: Option<PathBuf>,
        workspace_id: Option<WorkspaceId>,
        response: &str,
    ) -> (
        GeneratePluginTool<MockPluginRepo, MockArtifactRepo>,
        Arc<PluginManager<MockPluginRepo>>,
        tempfile::TempDir,
    ) {
        let llm = MockLlm::new(response);
        let generator = Arc::new(PluginGenerator::new(llm, "test-model"));
        let repo = MockPluginRepo::new();
        let manager = make_manager(repo);
        // Kept alive so the blob store's backing directory survives the test.
        let blob_dir = tempfile::tempdir().expect("blob tempdir");
        let blob_store = Arc::new(BlobStore::new(blob_dir.path().to_path_buf()));
        let tool = GeneratePluginTool::new(
            Arc::clone(&manager),
            GeneratePluginConfig {
                generator,
                blob_store,
                artifact_repo: None::<Arc<MockArtifactRepo>>,
                workspace_dir,
                workspace_id,
                conversation_id: None,
                user_id: UserId::new(),
                reserved_tool_names: vec![],
            },
        );
        (tool, manager, blob_dir)
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[test]
    fn metadata_correctness() {
        let (tool, _blob_dir) = make_tool(None);
        let meta = tool.metadata();
        assert_eq!(meta.name, "generate_plugin");
        assert!(meta.context_modifying);
        assert!(!meta.internal);
        assert!(meta.description.contains("plugin"));

        // Schema has required name and description fields.
        let required = meta.input_schema["required"]
            .as_array()
            .expect("required array");
        assert!(required.iter().any(|v| v.as_str() == Some("name")));
        assert!(required.iter().any(|v| v.as_str() == Some("description")));
    }

    #[test]
    fn metadata_has_kind_enum() {
        let (tool, _blob_dir) = make_tool(None);
        let meta = tool.metadata();
        let kind_schema = &meta.input_schema["properties"]["kind"];
        let enum_values = kind_schema["enum"].as_array().expect("enum array");
        assert!(enum_values.iter().any(|v| v.as_str() == Some("wasm")));
        assert!(enum_values.iter().any(|v| v.as_str() == Some("skill")));
    }

    #[tokio::test]
    async fn rejects_missing_name() {
        let (tool, _blob_dir) = make_tool(None);
        let result = tool
            .execute(serde_json::json!({"description": "test"}))
            .await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
        assert!(result.unwrap_err().to_string().contains("name"));
    }

    #[tokio::test]
    async fn rejects_empty_name() {
        let (tool, _blob_dir) = make_tool(None);
        let result = tool
            .execute(serde_json::json!({"name": "", "description": "test"}))
            .await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[tokio::test]
    async fn rejects_missing_description() {
        let (tool, _blob_dir) = make_tool(None);
        let result = tool.execute(serde_json::json!({"name": "test"})).await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
        assert!(result.unwrap_err().to_string().contains("description"));
    }

    #[tokio::test]
    async fn rejects_empty_description() {
        let (tool, _blob_dir) = make_tool(None);
        let result = tool
            .execute(serde_json::json!({"name": "test", "description": ""}))
            .await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[tokio::test]
    async fn rejects_unknown_kind() {
        let (tool, _blob_dir) = make_tool(Some(PathBuf::from("/tmp/test-ws")));
        let result = tool
            .execute(serde_json::json!({
                "name": "test",
                "description": "test",
                "kind": "unknown"
            }))
            .await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
        assert!(result.unwrap_err().to_string().contains("unknown"));
    }

    #[tokio::test]
    async fn skill_requires_workspace() {
        let (tool, _blob_dir) = make_tool(None);
        let result = tool
            .execute(serde_json::json!({
                "name": "test-skill",
                "description": "A test skill",
                "kind": "skill"
            }))
            .await
            .expect("should return Ok with is_error");
        assert!(result.is_error);
        assert!(result.content.contains("workspace"));
    }

    #[tokio::test]
    async fn wasm_requires_workspace() {
        let (tool, _blob_dir) = make_tool(None);
        let result = tool
            .execute(serde_json::json!({
                "name": "test-wasm",
                "description": "A test plugin",
                "kind": "wasm"
            }))
            .await
            .expect("should return Ok with is_error");
        assert!(result.is_error);
        assert!(result.content.contains("workspace"));
    }

    #[tokio::test]
    async fn skill_generation_saves_and_registers() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (tool, manager, _blob_dir) = make_tool_with_manager(
            Some(tmp.path().to_path_buf()),
            None,
            "---\nname: summariser\ndescription: Summarises text.\n---\n\n## Instructions\nSummarise the input.",
        );

        let result = tool
            .execute(serde_json::json!({
                "name": "summariser",
                "description": "Summarise long text",
                "kind": "skill"
            }))
            .await
            .expect("should succeed");

        assert!(!result.is_error);
        assert!(result.content.contains("summariser"));

        // Check that the file was written.
        let skill_path = tmp
            .path()
            .join(".sober")
            .join("skills")
            .join("summariser")
            .join("SKILL.md");
        assert!(skill_path.exists(), "skill file should exist");

        // Check that the plugin was registered by querying the repo.
        let repo = manager.repo();
        let created = repo.created_count();
        assert_eq!(created, 1);
        let last = repo.last_created().expect("should have created plugin");
        assert_eq!(last.name, "summariser");
        assert_eq!(last.kind, PluginKind::Skill);
        assert_eq!(last.origin, PluginOrigin::Agent);
    }

    #[tokio::test]
    async fn default_kind_is_skill() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (tool, manager, _blob_dir) = make_tool_with_manager(
            Some(tmp.path().to_path_buf()),
            None,
            "---\nname: test\ndescription: A test skill.\n---\n\n## Instructions\nTest skill content.",
        );

        // No "kind" field — should default to "skill".
        let result = tool
            .execute(serde_json::json!({
                "name": "test",
                "description": "A test skill"
            }))
            .await
            .expect("should succeed");

        assert!(!result.is_error);
        let last = manager
            .repo()
            .last_created()
            .expect("should have created plugin");
        assert_eq!(last.kind, PluginKind::Skill);
    }

    #[tokio::test]
    async fn workspace_scope_when_workspace_id_present() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ws_id = WorkspaceId::new();
        let (tool, manager, _blob_dir) = make_tool_with_manager(
            Some(tmp.path().to_path_buf()),
            Some(ws_id),
            "---\nname: scoped\ndescription: Scoped skill.\n---\n\n## Instructions\nScoped skill content.",
        );

        tool.execute(serde_json::json!({
            "name": "scoped",
            "description": "A workspace-scoped skill"
        }))
        .await
        .expect("should succeed");

        let last = manager
            .repo()
            .last_created()
            .expect("should have created plugin");
        assert_eq!(last.scope, PluginScope::Workspace);
        assert_eq!(last.workspace_id, Some(ws_id));
    }

    /// Verifies the regeneration detection: when a plugin with the same name
    /// already exists for this user, `list()` matches it by name and the
    /// `is_regeneration` flag would be set.
    ///
    /// This tests the mock's filtering logic which mirrors what the real
    /// `PluginRepo` implementation does in production.
    #[tokio::test]
    async fn mock_plugin_repo_detects_existing_plugin_by_name() {
        let user_id = UserId::new();
        let existing = Plugin {
            id: PluginId::new(),
            name: "my-plugin".to_owned(),
            kind: PluginKind::Wasm,
            version: Some("0.1.0".to_owned()),
            description: None,
            origin: PluginOrigin::Agent,
            scope: PluginScope::User,
            owner_id: Some(user_id),
            workspace_id: None,
            status: sober_core::types::enums::PluginStatus::Enabled,
            config: serde_json::json!({"wasm_blob_key": "old-key"}),
            installed_by: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let repo = MockPluginRepo::new().with_existing(existing.clone());

        // A filter that matches by name + scope + owner should find the plugin.
        let matches = repo
            .list(PluginFilter {
                name: Some("my-plugin".to_owned()),
                scope: Some(PluginScope::User),
                owner_id: Some(user_id),
                ..Default::default()
            })
            .await
            .expect("list should succeed");
        assert_eq!(matches.len(), 1, "should find the pre-seeded plugin");
        assert_eq!(matches[0].id, existing.id);

        // A filter with a different name should return nothing.
        let no_match = repo
            .list(PluginFilter {
                name: Some("other-plugin".to_owned()),
                scope: Some(PluginScope::User),
                owner_id: Some(user_id),
                ..Default::default()
            })
            .await
            .expect("list should succeed");
        assert!(
            no_match.is_empty(),
            "different name should not match the existing plugin"
        );
    }

    /// Verifies that `update_config` calls are tracked by the mock repo.
    #[tokio::test]
    async fn mock_plugin_repo_tracks_update_config() {
        let repo = MockPluginRepo::new();
        let plugin_id = PluginId::new();
        let new_config = serde_json::json!({"wasm_blob_key": "new-key"});

        repo.update_config(plugin_id, new_config.clone())
            .await
            .expect("update_config should succeed");

        assert_eq!(repo.config_update_count(), 1);
        let (id, cfg) = repo.last_config_update().expect("should have update");
        assert_eq!(id, plugin_id);
        assert_eq!(cfg, new_config);
    }

    /// Verifies that first-time skill creation does NOT call `update_config`.
    ///
    /// For WASM, the `is_regeneration` check happens after WASM compilation,
    /// which requires the wasm32-wasip1 toolchain and is tested at the
    /// integration level.  For skills the `register_plugin` path never calls
    /// `update_config`, confirming the first-time path is distinct.
    #[tokio::test]
    async fn first_time_skill_creation_does_not_call_update_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (tool, manager, _blob_dir) = make_tool_with_manager(
            Some(tmp.path().to_path_buf()),
            None,
            "---\nname: new-skill\ndescription: A brand new skill.\n---\n\n## Instructions\nDo something.",
        );

        tool.execute(serde_json::json!({
            "name": "new-skill",
            "description": "A brand new skill",
            "kind": "skill"
        }))
        .await
        .expect("first-time creation should succeed");

        let repo = manager.repo();
        assert_eq!(
            repo.config_update_count(),
            0,
            "update_config must not be called for first-time skill creation"
        );
        assert_eq!(repo.created_count(), 1, "plugin should have been created");
    }
}
