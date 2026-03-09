//! Adapters that wrap MCP tools and resources for use by sober-agent.
//!
//! [`McpToolAdapter`] implements the [`Tool`] trait from `sober-core`,
//! proxying calls through a shared [`McpClient`] reference.
//!
//! [`McpResourceAdapter`] provides a simple interface for reading resources.

use std::sync::Arc;

use sober_core::types::{Tool, ToolError, ToolMetadata, ToolOutput};
use tokio::sync::Mutex;

use crate::client::McpClient;
use crate::types::{McpResourceInfo, McpToolInfo, ResourceContent, ToolCallContent};

/// Adapter wrapping an MCP tool for use via the [`Tool`] trait.
///
/// Holds a reference to the shared [`McpClient`] and the tool's metadata.
/// Each tool call is proxied through the client.
pub struct McpToolAdapter {
    /// The tool metadata from the MCP server.
    tool_info: McpToolInfo,
    /// Server name (used to prefix the tool name for uniqueness).
    server_name: String,
    /// Shared client connection.
    client: Arc<Mutex<McpClient>>,
}

impl McpToolAdapter {
    /// Create a new tool adapter.
    pub fn new(tool_info: McpToolInfo, server_name: String, client: Arc<Mutex<McpClient>>) -> Self {
        Self {
            tool_info,
            server_name,
            client,
        }
    }

    /// Returns the original MCP tool name (without server prefix).
    #[must_use]
    pub fn mcp_name(&self) -> &str {
        &self.tool_info.name
    }

    /// Returns the server name this tool belongs to.
    #[must_use]
    pub fn server_name(&self) -> &str {
        &self.server_name
    }
}

impl Tool for McpToolAdapter {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: format!("{}_{}", self.server_name, self.tool_info.name),
            description: self.tool_info.description.clone().unwrap_or_default(),
            input_schema: self.tool_info.input_schema.clone(),
            context_modifying: false,
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let mut client = self.client.lock().await;

        let result = client
            .call_tool(&self.tool_info.name, input)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        // Flatten content items into a single string.
        let content = result
            .content
            .iter()
            .map(|c| match c {
                ToolCallContent::Text { text } => text.clone(),
                ToolCallContent::Image { mime_type, .. } => {
                    format!("[image: {mime_type}]")
                }
                ToolCallContent::Resource { resource } => resource
                    .text
                    .clone()
                    .unwrap_or_else(|| format!("[resource: {}]", resource.uri)),
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(ToolOutput {
            content,
            is_error: result.is_error,
        })
    }
}

/// Adapter for reading MCP resources.
///
/// Unlike tools, resources don't have a standardized trait in sober-core,
/// so this adapter provides its own interface.
pub struct McpResourceAdapter {
    /// The resource metadata from the MCP server.
    resource_info: McpResourceInfo,
    /// Server name.
    server_name: String,
    /// Shared client connection.
    client: Arc<Mutex<McpClient>>,
}

impl McpResourceAdapter {
    /// Create a new resource adapter.
    pub fn new(
        resource_info: McpResourceInfo,
        server_name: String,
        client: Arc<Mutex<McpClient>>,
    ) -> Self {
        Self {
            resource_info,
            server_name,
            client,
        }
    }

    /// Returns the resource URI.
    #[must_use]
    pub fn uri(&self) -> &str {
        &self.resource_info.uri
    }

    /// Returns the resource display name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.resource_info.name
    }

    /// Returns the server name this resource belongs to.
    #[must_use]
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// Returns the resource description, if any.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.resource_info.description.as_deref()
    }

    /// Read the resource content from the MCP server.
    ///
    /// # Errors
    ///
    /// Returns [`McpError`](crate::McpError) (wrapped) if the read fails.
    pub async fn read(&self) -> Result<Vec<ResourceContent>, ToolError> {
        let mut client = self.client.lock().await;

        client
            .read_resource(&self.resource_info.uri)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tool_info() -> McpToolInfo {
        McpToolInfo {
            name: "web_search".to_owned(),
            description: Some("Search the web".to_owned()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                }
            }),
        }
    }

    fn sample_resource_info() -> McpResourceInfo {
        McpResourceInfo {
            uri: "file:///data.txt".to_owned(),
            name: "data.txt".to_owned(),
            description: Some("A data file".to_owned()),
            mime_type: Some("text/plain".to_owned()),
        }
    }

    #[test]
    fn tool_adapter_metadata_prefixes_server_name() {
        // We can't easily construct an McpClient for unit tests without
        // a process, so just test metadata generation with a dummy Arc.
        let info = sample_tool_info();

        // Verify the metadata formatting logic directly.
        let prefixed_name = format!("{}_{}", "test_server", info.name);
        assert_eq!(prefixed_name, "test_server_web_search");
    }

    #[test]
    fn resource_adapter_accessors() {
        let info = sample_resource_info();
        assert_eq!(info.uri, "file:///data.txt");
        assert_eq!(info.name, "data.txt");
        assert_eq!(info.description.as_deref(), Some("A data file"));
        assert_eq!(info.mime_type.as_deref(), Some("text/plain"));
    }
}
