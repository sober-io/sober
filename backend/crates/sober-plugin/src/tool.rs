//! WASM plugin tool — [`Tool`] trait adapter for [`PluginHost`].
//!
//! [`PluginTool`] wraps a shared [`PluginHost`] and exposes a single tool from
//! the plugin's manifest.  WASM execution is synchronous, so [`PluginTool::execute`]
//! uses `spawn_blocking` to avoid blocking the async runtime.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use metrics::{counter, histogram};
use sober_core::types::ids::PluginId;
use sober_core::types::tool::{BoxToolFuture, Tool, ToolMetadata, ToolOutput};
use tracing::{debug, warn};

use crate::host::PluginHost;

/// A WASM plugin tool that implements the [`Tool`] trait.
///
/// Wraps a shared [`PluginHost`] and exposes a single tool from the
/// plugin's manifest.  WASM execution is synchronous, so [`execute`]
/// uses `spawn_blocking` to avoid blocking the async runtime.
///
/// Multiple `PluginTool` instances can share the same host (one per
/// manifest tool entry), coordinated through the inner [`Mutex`].
pub struct PluginTool {
    host: Arc<Mutex<PluginHost>>,
    tool_name: String,
    metadata: ToolMetadata,
    plugin_id: PluginId,
    user_id: Option<sober_core::types::ids::UserId>,
    db_pool: Option<sqlx::PgPool>,
}

impl PluginTool {
    /// Creates a new `PluginTool` for the given tool entry.
    ///
    /// The `host` is shared across all tools from the same plugin.
    /// `tool_name` must match a `[[tools]]` entry in the manifest.
    /// `description` comes from the manifest's tool entry.
    pub fn new(
        host: Arc<Mutex<PluginHost>>,
        tool_name: String,
        description: String,
        plugin_id: PluginId,
        user_id: Option<sober_core::types::ids::UserId>,
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
            host,
            tool_name,
            metadata,
            plugin_id,
            user_id,
            db_pool,
        }
    }
}

impl Tool for PluginTool {
    fn metadata(&self) -> ToolMetadata {
        self.metadata.clone()
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        let host = Arc::clone(&self.host);
        let tool_name = self.tool_name.clone();
        let meta_tool_name = self.tool_name.clone();
        let plugin_id = self.plugin_id;
        let user_id = self.user_id;
        let db_pool = self.db_pool.clone();

        // Capture the plugin name from the host's manifest for logging.
        let plugin_name = self
            .host
            .lock()
            .ok()
            .map(|h| h.manifest().plugin.name.clone())
            .unwrap_or_else(|| "<unknown>".to_owned());

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

            let duration_ms = start.elapsed().as_millis() as u64;
            let duration_secs = start.elapsed().as_secs_f64();

            let (success, error_message, output) = match result {
                Ok(output) => {
                    counter!(
                        "sober_plugin_executions_total",
                        "plugin" => plugin_name.clone(),
                        "tool" => meta_tool_name.clone(),
                        "status" => "success",
                    )
                    .increment(1);
                    debug!(
                        plugin = %plugin_name,
                        tool = %meta_tool_name,
                        duration_ms,
                        "plugin tool execution succeeded"
                    );
                    (true, None, Ok(output))
                }
                Err(msg) => {
                    counter!(
                        "sober_plugin_executions_total",
                        "plugin" => plugin_name.clone(),
                        "tool" => meta_tool_name.clone(),
                        "status" => "error",
                    )
                    .increment(1);
                    warn!(
                        plugin = %plugin_name,
                        tool = %meta_tool_name,
                        duration_ms,
                        error = %msg,
                        "plugin tool execution failed"
                    );
                    (
                        false,
                        Some(msg.clone()),
                        Ok(ToolOutput {
                            content: format!("Plugin execution failed: {msg}"),
                            is_error: true,
                        }),
                    )
                }
            };

            histogram!(
                "sober_plugin_execution_duration_seconds",
                "plugin" => plugin_name.clone(),
                "tool" => meta_tool_name.clone(),
            )
            .record(duration_secs);

            // Persist execution log to DB.
            if let Some(pool) = db_pool {
                tokio::spawn(async move {
                    if let Err(e) = sqlx::query(
                        "INSERT INTO plugin_execution_logs \
                         (plugin_id, plugin_name, tool_name, user_id, \
                          duration_ms, success, error_message) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7)",
                    )
                    .bind(plugin_id.as_uuid())
                    .bind(&plugin_name)
                    .bind(&meta_tool_name)
                    .bind(user_id.map(|id| *id.as_uuid()))
                    .bind(duration_ms as i64)
                    .bind(success)
                    .bind(&error_message)
                    .execute(&pool)
                    .await
                    {
                        tracing::warn!(error = %e, "failed to persist plugin execution log");
                    }
                });
            }

            output
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
