//! Agent tool for generating plugins via LLM.
//!
//! Exposes a `/generate-plugin` tool that delegates to [`PluginGenerator`] from
//! `sober-plugin-gen` to create WASM plugins or markdown skills from a
//! natural-language description.

use std::path::PathBuf;
use std::sync::Arc;

use sober_core::types::enums::{ArtifactKind, PluginKind, PluginOrigin, PluginScope};
use sober_core::types::ids::{ConversationId, UserId, WorkspaceId};
use sober_core::types::input::CreateArtifact;
use sober_core::types::repo::{ArtifactRepo, PluginRepo};
use sober_core::types::tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
use sober_plugin::PluginManager;
use sober_plugin::registry::InstallRequest;
use sober_plugin_gen::PluginGenerator;
use sober_workspace::BlobStore;

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
}

impl<R: PluginRepo, A: ArtifactRepo> GeneratePluginTool<R, A> {
    /// Creates a new generate-plugin tool.
    ///
    /// `workspace_dir` is the directory where generated plugin source files
    /// will be saved for human inspection. If `None`, the tool returns a
    /// message asking the user to set up a workspace first.
    ///
    /// Generated WASM bytes are stored in `blob_store` content-addressed
    /// rather than written to the filesystem, so they survive workspace moves.
    /// When `artifact_repo` and `workspace_id` are both provided the blob
    /// is also registered as a workspace artifact.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        generator: Arc<PluginGenerator>,
        plugin_manager: Arc<PluginManager<R>>,
        blob_store: Arc<BlobStore>,
        artifact_repo: Option<Arc<A>>,
        workspace_dir: Option<PathBuf>,
        workspace_id: Option<WorkspaceId>,
        conversation_id: Option<ConversationId>,
        user_id: UserId,
    ) -> Self {
        Self {
            generator,
            plugin_manager,
            blob_store,
            artifact_repo,
            workspace_dir,
            workspace_id,
            conversation_id,
            user_id,
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
                        "description": "Required capabilities for WASM plugins: key_value, network, secret_read."
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

        match kind {
            "wasm" => self.generate_wasm(name, description, &input).await,
            "skill" => self.generate_skill(name, description).await,
            other => Err(ToolError::InvalidInput(format!(
                "unknown plugin kind '{other}', expected 'wasm' or 'skill'"
            ))),
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
    /// The WASM binary is stored in [`BlobStore`] (content-addressed by SHA-256).
    /// Source and manifest files are written to the workspace for human
    /// inspection, but the WASM binary itself lives only in the blob store.
    /// When a workspace context is available the blob is also registered as a
    /// [`Document`](ArtifactKind::Document) artifact.
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
        let blob_key = self
            .blob_store
            .store(&generated.wasm_bytes)
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("failed to store WASM in blob store: {e}"))
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

        // Parse the manifest for the audit pipeline.
        let manifest = sober_plugin::PluginManifest::from_toml(&generated.manifest)
            .map_err(|e| ToolError::ExecutionFailed(format!("invalid manifest: {e}")))?;

        // Register the plugin through the audit pipeline.
        // Plugin config references blob_key instead of a filesystem wasm_path.
        self.register_plugin(
            name,
            description,
            PluginKind::Wasm,
            serde_json::json!({
                "blob_key": blob_key,
                "source_path": plugin_dir.join("src_lib.rs").to_string_lossy(),
                "manifest_toml": generated.manifest,
            }),
            Some(manifest),
            Some(generated.wasm_bytes),
        )
        .await?;

        // Create an artifact record when a workspace context is available.
        if let (Some(ws_id), Some(artifact_repo)) = (self.workspace_id, self.artifact_repo.as_ref())
        {
            let create_input = CreateArtifact {
                workspace_id: ws_id,
                user_id: self.user_id,
                kind: ArtifactKind::Document,
                title: format!("plugin:{name}"),
                description: Some(description.to_owned()),
                storage_type: "blob".to_owned(),
                blob_key: Some(blob_key.clone()),
                created_by: None, // None = agent-created
                conversation_id: self.conversation_id,
                ..Default::default()
            };

            // Non-fatal: log a warning if artifact creation fails, but don't
            // fail the whole tool invocation — the plugin is already registered.
            if let Err(e) = artifact_repo.create(create_input).await {
                tracing::warn!(
                    plugin_name = name,
                    blob_key = %blob_key,
                    error = %e,
                    "failed to create artifact record for generated WASM plugin"
                );
            }
        }

        Ok(ToolOutput {
            content: format!(
                "Generated WASM plugin '{name}'.\n\
                 Source saved to: {}\n\
                 WASM stored as blob: {blob_key}\n\
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
    }

    impl MockPluginRepo {
        fn new() -> Self {
            Self {
                created: Mutex::new(vec![]),
            }
        }

        fn created_count(&self) -> usize {
            self.created.lock().expect("lock").len()
        }

        fn last_created(&self) -> Option<CreatePlugin> {
            self.created.lock().expect("lock").last().cloned()
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
            _filter: PluginFilter,
        ) -> impl std::future::Future<Output = Result<Vec<Plugin>, AppError>> + Send {
            async { Ok(vec![]) }
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
    /// Returns the tool and the `TempDir` so the caller can keep it alive
    /// for the duration of the test.
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
        let blob_tmp = tempfile::tempdir().expect("blob tempdir");
        let blob_store = Arc::new(BlobStore::new(blob_tmp.path().to_path_buf()));
        let tool = GeneratePluginTool::new(
            generator,
            manager,
            blob_store,
            None::<Arc<MockArtifactRepo>>,
            workspace_dir,
            None,
            None,
            UserId::new(),
        );
        (tool, blob_tmp)
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
        let blob_tmp = tempfile::tempdir().expect("blob tempdir");
        let blob_store = Arc::new(BlobStore::new(blob_tmp.path().to_path_buf()));
        let tool = GeneratePluginTool::new(
            generator,
            Arc::clone(&manager),
            blob_store,
            None::<Arc<MockArtifactRepo>>,
            workspace_dir,
            workspace_id,
            None,
            UserId::new(),
        );
        (tool, manager, blob_tmp)
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[test]
    fn metadata_correctness() {
        let (tool, _blob_tmp) = make_tool(None);
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
        let (tool, _blob_tmp) = make_tool(None);
        let meta = tool.metadata();
        let kind_schema = &meta.input_schema["properties"]["kind"];
        let enum_values = kind_schema["enum"].as_array().expect("enum array");
        assert!(enum_values.iter().any(|v| v.as_str() == Some("wasm")));
        assert!(enum_values.iter().any(|v| v.as_str() == Some("skill")));
    }

    #[tokio::test]
    async fn rejects_missing_name() {
        let (tool, _blob_tmp) = make_tool(None);
        let result = tool
            .execute(serde_json::json!({"description": "test"}))
            .await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
        assert!(result.unwrap_err().to_string().contains("name"));
    }

    #[tokio::test]
    async fn rejects_empty_name() {
        let (tool, _blob_tmp) = make_tool(None);
        let result = tool
            .execute(serde_json::json!({"name": "", "description": "test"}))
            .await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[tokio::test]
    async fn rejects_missing_description() {
        let (tool, _blob_tmp) = make_tool(None);
        let result = tool.execute(serde_json::json!({"name": "test"})).await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
        assert!(result.unwrap_err().to_string().contains("description"));
    }

    #[tokio::test]
    async fn rejects_empty_description() {
        let (tool, _blob_tmp) = make_tool(None);
        let result = tool
            .execute(serde_json::json!({"name": "test", "description": ""}))
            .await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[tokio::test]
    async fn rejects_unknown_kind() {
        let (tool, _blob_tmp) = make_tool(Some(PathBuf::from("/tmp/test-ws")));
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
        let (tool, _blob_tmp) = make_tool(None);
        let result = tool
            .execute(serde_json::json!({
                "name": "test-skill",
                "description": "A test skill",
                "kind": "skill"
            }))
            .await;
        assert!(matches!(result, Err(ToolError::ExecutionFailed(_))));
        assert!(result.unwrap_err().to_string().contains("workspace"));
    }

    #[tokio::test]
    async fn wasm_requires_workspace() {
        let (tool, _blob_tmp) = make_tool(None);
        let result = tool
            .execute(serde_json::json!({
                "name": "test-wasm",
                "description": "A test plugin",
                "kind": "wasm"
            }))
            .await;
        assert!(matches!(result, Err(ToolError::ExecutionFailed(_))));
        assert!(result.unwrap_err().to_string().contains("workspace"));
    }

    #[tokio::test]
    async fn skill_generation_saves_and_registers() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (tool, manager, _blob_tmp) = make_tool_with_manager(
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
        let (tool, manager, _blob_tmp) = make_tool_with_manager(
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
        let (tool, manager, _blob_tmp) = make_tool_with_manager(
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
}
