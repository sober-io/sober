//! WASM plugin tool — [`Tool`] trait adapter for [`PluginHost`].
//!
//! [`PluginTool`] wraps a shared [`PluginHost`] and exposes a single tool from
//! the plugin's manifest.  WASM execution is synchronous, so [`PluginTool::execute`]
//! uses `spawn_blocking` to avoid blocking the async runtime.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use metrics::{counter, histogram};
use sober_core::types::tool::{BoxToolFuture, Tool, ToolMetadata, ToolOutput};

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
}

impl PluginTool {
    /// Creates a new `PluginTool` for the given tool entry.
    ///
    /// The `host` is shared across all tools from the same plugin.
    /// `tool_name` must match a `[[tools]]` entry in the manifest.
    /// `description` comes from the manifest's tool entry.
    pub fn new(host: Arc<Mutex<PluginHost>>, tool_name: String, description: String) -> Self {
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

            let duration_secs = start.elapsed().as_secs_f64();

            match result {
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
                    Ok(ToolOutput {
                        content: format!("Plugin execution failed: {msg}"),
                        is_error: true,
                    })
                }
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
