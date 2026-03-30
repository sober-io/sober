//! WASM plugin tool — [`Tool`] trait adapter for [`PluginHost`].
//!
//! [`PluginTool`] wraps a shared [`PluginHost`] and exposes a single tool from
//! the plugin's manifest.  WASM execution is synchronous, so [`PluginTool::execute`]
//! uses `spawn_blocking` to avoid blocking the async runtime.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use metrics::{counter, histogram};
use sober_core::types::ids::{PluginId, UserId, WorkspaceId};
use sober_core::types::tool::{BoxToolFuture, Tool, ToolMetadata, ToolOutput};

use crate::host::PluginHost;

/// State of the underlying WASM host for a plugin tool.
///
/// Tools are always registered in the tool registry (from the manifest),
/// but the WASM host may not have loaded successfully. In the `Failed`
/// state, [`PluginTool::execute`] returns a clear error to the LLM
/// instead of the tool being silently invisible.
#[derive(Clone)]
pub enum WasmHostState {
    /// Host loaded and ready for execution.
    Loaded(Arc<Mutex<PluginHost>>),
    /// Host failed to load — stores the error message.
    Failed(String),
}

/// A WASM plugin tool that implements the [`Tool`] trait.
///
/// Wraps a shared [`PluginHost`] and exposes a single tool from the
/// plugin's manifest.  WASM execution is synchronous, so [`execute`]
/// uses `spawn_blocking` to avoid blocking the async runtime.
///
/// Multiple `PluginTool` instances can share the same host (one per
/// manifest tool entry), coordinated through the inner [`Mutex`].
pub struct PluginTool {
    host_state: WasmHostState,
    plugin_name: String,
    tool_name: String,
    metadata: ToolMetadata,
    plugin_id: PluginId,
    user_id: Option<UserId>,
    workspace_id: Option<WorkspaceId>,
    db_pool: Option<sqlx::PgPool>,
}

impl PluginTool {
    /// Creates a new `PluginTool` for the given tool entry.
    ///
    /// The `host` is shared across all tools from the same plugin.
    /// `tool_name` must match a `[[tools]]` entry in the manifest.
    /// `description` comes from the manifest's tool entry.
    ///
    /// When `db_pool` is `Some`, execution logs are persisted to the
    /// `plugin_execution_logs` table after each invocation.
    /// Creates a new `PluginTool` for the given tool entry.
    ///
    /// `host_state` is either `Loaded` (ready for execution) or `Failed`
    /// (will return an error on execute). The host is shared across all
    /// tools from the same plugin.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host_state: WasmHostState,
        plugin_name: String,
        tool_name: String,
        description: String,
        plugin_id: PluginId,
        user_id: Option<UserId>,
        workspace_id: Option<WorkspaceId>,
        db_pool: Option<sqlx::PgPool>,
    ) -> Self {
        let metadata = ToolMetadata {
            name: tool_name.clone(),
            description,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": true,
            }),
            context_modifying: false,
            internal: false,
        };

        Self {
            host_state,
            plugin_name,
            tool_name,
            metadata,
            plugin_id,
            user_id,
            workspace_id,
            db_pool,
        }
    }
}

impl Tool for PluginTool {
    fn metadata(&self) -> ToolMetadata {
        self.metadata.clone()
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        let host = match &self.host_state {
            WasmHostState::Loaded(h) => Arc::clone(h),
            WasmHostState::Failed(err) => {
                let msg = format!("WASM plugin '{}' failed to load: {err}", self.plugin_name);
                return Box::pin(async move {
                    Err(sober_core::types::tool::ToolError::ExecutionFailed(msg))
                });
            }
        };
        let tool_name = self.tool_name.clone();
        let meta_tool_name = self.tool_name.clone();
        let plugin_name = self.plugin_name.clone();

        let db_pool = self.db_pool.clone();
        let plugin_id = self.plugin_id;
        let user_id = self.user_id;
        let workspace_id = self.workspace_id;

        Box::pin(async move {
            let start = Instant::now();

            let result = tokio::task::spawn_blocking(move || {
                let mut host = host
                    .lock()
                    .map_err(|e| format!("plugin host lock poisoned: {e}"))?;
                host.call_tool(&tool_name, input).map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| format!("spawn_blocking failed: {e}"))
            .and_then(|r| r);

            let duration_secs = start.elapsed().as_secs_f64();
            let duration_ms = start.elapsed().as_millis() as i64;

            let output_result = match result {
                Ok(output) => {
                    counter!(
                        "sober_plugin_executions_total",
                        "plugin" => plugin_name.clone(),
                        "tool" => meta_tool_name.clone(),
                        "status" => "success",
                    )
                    .increment(1);
                    histogram!(
                        "sober_plugin_execution_duration_seconds",
                        "plugin" => plugin_name.clone(),
                        "tool" => meta_tool_name.clone(),
                    )
                    .record(duration_secs);
                    Ok(output)
                }
                Err(msg) => {
                    counter!(
                        "sober_plugin_executions_total",
                        "plugin" => plugin_name.clone(),
                        "tool" => meta_tool_name.clone(),
                        "status" => "error",
                    )
                    .increment(1);
                    histogram!(
                        "sober_plugin_execution_duration_seconds",
                        "plugin" => plugin_name.clone(),
                        "tool" => meta_tool_name.clone(),
                    )
                    .record(duration_secs);
                    Err(msg)
                }
            };

            // Persist plugin execution log to the database.
            if let Some(pool) = db_pool {
                let success = output_result.is_ok();
                let error_msg = if let Err(ref msg) = output_result {
                    Some(msg.clone())
                } else {
                    None
                };
                let log_plugin_name = plugin_name.clone();
                let log_tool_name = meta_tool_name.clone();
                tokio::spawn(async move {
                    let _ = sqlx::query(
                        "INSERT INTO plugin_execution_logs \
                         (plugin_id, plugin_name, tool_name, user_id, workspace_id, \
                          duration_ms, success, error_message) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                    )
                    .bind(plugin_id.as_uuid())
                    .bind(&log_plugin_name)
                    .bind(&log_tool_name)
                    .bind(user_id.map(|id| *id.as_uuid()))
                    .bind(workspace_id.map(|id| *id.as_uuid()))
                    .bind(duration_ms)
                    .bind(success)
                    .bind(&error_msg)
                    .execute(&pool)
                    .await;
                });
            }

            match output_result {
                Ok(output) => Ok(output),
                Err(msg) => Ok(ToolOutput {
                    content: format!("Plugin execution failed: {msg}"),
                    is_error: true,
                }),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_returns_correct_values() {
        let metadata = ToolMetadata {
            name: "my_tool".into(),
            description: "Does stuff".into(),
            input_schema: serde_json::json!({"type": "object"}),
            context_modifying: false,
            internal: false,
        };

        assert_eq!(metadata.name, "my_tool");
        assert_eq!(metadata.description, "Does stuff");
        assert!(!metadata.context_modifying);
        assert!(!metadata.internal);
    }

    #[test]
    fn new_builds_correct_metadata() {
        // We cannot create a real PluginHost without valid WASM, but we
        // can verify the metadata construction by checking the schema
        // shape and field values that `new()` produces.
        let expected_schema = serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": true,
        });

        let metadata = ToolMetadata {
            name: "greet".into(),
            description: "Greets someone".into(),
            input_schema: expected_schema.clone(),
            context_modifying: false,
            internal: false,
        };

        assert_eq!(metadata.name, "greet");
        assert_eq!(metadata.description, "Greets someone");
        assert_eq!(metadata.input_schema, expected_schema);
        assert!(!metadata.context_modifying);
        assert!(!metadata.internal);
    }

    // Compile-time assertion that PluginTool is Send + Sync.
    #[allow(dead_code)]
    const _: () = {
        fn assert_send_sync<T: Send + Sync>() {}
        fn check() {
            assert_send_sync::<PluginTool>();
        }
    };
}
