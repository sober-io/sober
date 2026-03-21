//! Agent tool for generating plugins via LLM.
//!
//! Exposes a `/generate-plugin` tool that delegates to [`PluginGenerator`] from
//! `sober-plugin-gen` to create WASM plugins or markdown skills from a
//! natural-language description.

use std::path::PathBuf;
use std::sync::Arc;

use sober_core::types::enums::{PluginKind, PluginOrigin, PluginScope, PluginStatus};
use sober_core::types::ids::{UserId, WorkspaceId};
use sober_core::types::input::CreatePlugin;
use sober_core::types::repo::PluginRepo;
use sober_core::types::tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
use sober_plugin::PluginManager;
use sober_plugin_gen::PluginGenerator;

/// Built-in tool that generates plugins (WASM or skill) via LLM.
///
/// Delegates to [`PluginGenerator`] for the actual generation work, saves the
/// output files to the workspace, and registers the plugin in the database
/// via the [`PluginManager`]'s repository.
pub struct GeneratePluginTool<R: PluginRepo> {
    generator: Arc<PluginGenerator>,
    plugin_manager: Arc<PluginManager<R>>,
    workspace_dir: Option<PathBuf>,
    workspace_id: Option<WorkspaceId>,
    user_id: UserId,
}

impl<R: PluginRepo> GeneratePluginTool<R> {
    /// Creates a new generate-plugin tool.
    ///
    /// `workspace_dir` is the directory where generated plugin files will be
    /// saved. If `None`, the tool returns a message asking the user to set up
    /// a workspace first.
    pub fn new(
        generator: Arc<PluginGenerator>,
        plugin_manager: Arc<PluginManager<R>>,
        workspace_dir: Option<PathBuf>,
        workspace_id: Option<WorkspaceId>,
        user_id: UserId,
    ) -> Self {
        Self {
            generator,
            plugin_manager,
            workspace_dir,
            workspace_id,
            user_id,
        }
    }
}

impl<R: PluginRepo + 'static> Tool for GeneratePluginTool<R> {
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

impl<R: PluginRepo + 'static> GeneratePluginTool<R> {
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

    /// Registers a plugin in the database via the plugin manager's repo.
    async fn register_plugin(
        &self,
        name: &str,
        description: &str,
        kind: PluginKind,
        config: serde_json::Value,
    ) -> Result<sober_core::types::domain::Plugin, ToolError> {
        let scope = if self.workspace_id.is_some() {
            PluginScope::Workspace
        } else {
            PluginScope::User
        };

        let create_input = CreatePlugin {
            name: name.to_owned(),
            kind,
            version: Some("0.1.0".to_owned()),
            description: Some(description.to_owned()),
            origin: PluginOrigin::Agent,
            scope,
            owner_id: Some(self.user_id),
            workspace_id: self.workspace_id,
            status: PluginStatus::Enabled,
            config,
            installed_by: Some(self.user_id),
        };

        self.plugin_manager
            .repo()
            .create(create_input)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to register plugin: {e}")))
    }

    /// Generates a WASM plugin and saves it to the workspace.
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

        // Save files to workspace under .sober/plugins/<name>/
        let plugin_dir = workspace_dir.join(".sober").join("plugins").join(name);
        tokio::fs::create_dir_all(&plugin_dir)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to create plugin dir: {e}")))?;

        let wasm_path = plugin_dir.join(format!("{name}.wasm"));
        tokio::fs::write(&wasm_path, &generated.wasm_bytes)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to write WASM file: {e}")))?;

        tokio::fs::write(plugin_dir.join("plugin.toml"), &generated.manifest)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to write manifest: {e}")))?;

        tokio::fs::write(plugin_dir.join("src_lib.rs"), &generated.source)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to write source: {e}")))?;

        // Register the plugin in the database.
        let plugin = self
            .register_plugin(
                name,
                description,
                PluginKind::Wasm,
                serde_json::json!({
                    "wasm_path": wasm_path.to_string_lossy(),
                    "manifest_toml": generated.manifest,
                }),
            )
            .await?;

        Ok(ToolOutput {
            content: format!(
                "Generated WASM plugin '{name}' (id: {}).\n\
                 Files saved to: {}\n\
                 Capabilities: {}\n\
                 Status: enabled",
                plugin.id,
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

        // Save skill markdown to .sober/skills/<name>.md
        let skills_dir = workspace_dir.join(".sober").join("skills");
        tokio::fs::create_dir_all(&skills_dir)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to create skills dir: {e}")))?;

        let skill_path = skills_dir.join(format!("{name}.md"));
        tokio::fs::write(&skill_path, &skill_content)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to write skill file: {e}")))?;

        // Register the skill plugin in the database.
        let plugin = self
            .register_plugin(
                name,
                description,
                PluginKind::Skill,
                serde_json::json!({
                    "skill_path": skill_path.to_string_lossy(),
                }),
            )
            .await?;

        Ok(ToolOutput {
            content: format!(
                "Generated skill '{name}' (id: {}).\n\
                 Saved to: {}\n\
                 Status: enabled\n\n\
                 The skill will be available in the next conversation turn.",
                plugin.id,
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
    use sober_core::types::domain::{Plugin, PluginAuditLog};
    use sober_core::types::ids::PluginId;
    use sober_core::types::input::{CreatePluginAuditLog, PluginFilter};
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
            _input: CreatePluginAuditLog,
        ) -> impl std::future::Future<Output = Result<PluginAuditLog, AppError>> + Send {
            async { Err(AppError::Internal("not implemented".into())) }
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
    // Helper
    // -----------------------------------------------------------------------

    fn make_manager(repo: MockPluginRepo) -> Arc<PluginManager<MockPluginRepo>> {
        let pool = McpPool::new(McpConfig::default());
        let loader = Arc::new(SkillLoader::new(Duration::from_secs(300)));
        Arc::new(PluginManager::new(repo, pool, loader))
    }

    fn make_tool(workspace_dir: Option<PathBuf>) -> GeneratePluginTool<MockPluginRepo> {
        let llm = MockLlm::new("# Skill: Test\n\nA test skill.");
        let generator = Arc::new(PluginGenerator::new(llm, "test-model"));
        let manager = make_manager(MockPluginRepo::new());
        GeneratePluginTool::new(generator, manager, workspace_dir, None, UserId::new())
    }

    fn make_tool_with_manager(
        workspace_dir: Option<PathBuf>,
        workspace_id: Option<WorkspaceId>,
        response: &str,
    ) -> (
        GeneratePluginTool<MockPluginRepo>,
        Arc<PluginManager<MockPluginRepo>>,
    ) {
        let llm = MockLlm::new(response);
        let generator = Arc::new(PluginGenerator::new(llm, "test-model"));
        let repo = MockPluginRepo::new();
        let manager = make_manager(repo);
        let tool = GeneratePluginTool::new(
            generator,
            Arc::clone(&manager),
            workspace_dir,
            workspace_id,
            UserId::new(),
        );
        (tool, manager)
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[test]
    fn metadata_correctness() {
        let tool = make_tool(None);
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
        let tool = make_tool(None);
        let meta = tool.metadata();
        let kind_schema = &meta.input_schema["properties"]["kind"];
        let enum_values = kind_schema["enum"].as_array().expect("enum array");
        assert!(enum_values.iter().any(|v| v.as_str() == Some("wasm")));
        assert!(enum_values.iter().any(|v| v.as_str() == Some("skill")));
    }

    #[tokio::test]
    async fn rejects_missing_name() {
        let tool = make_tool(None);
        let result = tool
            .execute(serde_json::json!({"description": "test"}))
            .await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
        assert!(result.unwrap_err().to_string().contains("name"));
    }

    #[tokio::test]
    async fn rejects_empty_name() {
        let tool = make_tool(None);
        let result = tool
            .execute(serde_json::json!({"name": "", "description": "test"}))
            .await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[tokio::test]
    async fn rejects_missing_description() {
        let tool = make_tool(None);
        let result = tool.execute(serde_json::json!({"name": "test"})).await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
        assert!(result.unwrap_err().to_string().contains("description"));
    }

    #[tokio::test]
    async fn rejects_empty_description() {
        let tool = make_tool(None);
        let result = tool
            .execute(serde_json::json!({"name": "test", "description": ""}))
            .await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[tokio::test]
    async fn rejects_unknown_kind() {
        let tool = make_tool(Some(PathBuf::from("/tmp/test-ws")));
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
        let tool = make_tool(None);
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
        let tool = make_tool(None);
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
        let (tool, manager) = make_tool_with_manager(
            Some(tmp.path().to_path_buf()),
            None,
            "# Skill: Summariser\n\nSummarises text.",
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
            .join("summariser.md");
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
        let (tool, manager) = make_tool_with_manager(
            Some(tmp.path().to_path_buf()),
            None,
            "# Skill: Test\n\nTest skill content.",
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
        let (tool, manager) = make_tool_with_manager(
            Some(tmp.path().to_path_buf()),
            Some(ws_id),
            "# Skill: Scoped\n\nScoped skill.",
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
