//! MCP domain types for tools, resources, and server capabilities.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::{Deserialize, Serialize};

/// Information about a tool provided by an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    /// Tool name (unique within the server).
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// JSON Schema for the tool's input parameters.
    #[serde(default, rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// Information about a resource provided by an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceInfo {
    /// Resource URI.
    pub uri: String,
    /// Human-readable name.
    pub name: String,
    /// Description of the resource.
    #[serde(default)]
    pub description: Option<String>,
    /// MIME type of the resource content.
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<String>,
}

/// Content returned when reading a resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContent {
    /// Resource URI this content belongs to.
    pub uri: String,
    /// MIME type of the content.
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<String>,
    /// Text content (mutually exclusive with `blob`).
    #[serde(default)]
    pub text: Option<String>,
    /// Base64-encoded binary content (mutually exclusive with `text`).
    #[serde(default)]
    pub blob: Option<String>,
}

impl ResourceContent {
    /// Decode the blob field from base64, if present.
    ///
    /// Returns `None` if no blob is set.
    ///
    /// # Errors
    ///
    /// Returns an error string if the base64 decoding fails.
    pub fn decode_blob(&self) -> Result<Option<Vec<u8>>, String> {
        match &self.blob {
            Some(b64) => BASE64_STANDARD
                .decode(b64)
                .map(Some)
                .map_err(|e| format!("base64 decode failed: {e}")),
            None => Ok(None),
        }
    }
}

/// Result from calling a tool on the MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    /// Content items returned by the tool.
    pub content: Vec<ToolCallContent>,
    /// Whether this result represents an error condition.
    #[serde(default, rename = "isError")]
    pub is_error: bool,
}

/// A content item within a tool call result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolCallContent {
    /// Text content.
    #[serde(rename = "text")]
    Text {
        /// The text content.
        text: String,
    },
    /// Image content.
    #[serde(rename = "image")]
    Image {
        /// Base64-encoded image data.
        data: String,
        /// MIME type (e.g. "image/png").
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    /// Embedded resource content.
    #[serde(rename = "resource")]
    Resource {
        /// The resource content.
        resource: ResourceContent,
    },
}

/// Information about an MCP server from the initialize handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Server name.
    pub name: String,
    /// Server version.
    #[serde(default)]
    pub version: Option<String>,
}

/// Capabilities advertised by the MCP server.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// Whether the server supports tool operations.
    #[serde(default)]
    pub tools: Option<serde_json::Value>,
    /// Whether the server supports resource operations.
    #[serde(default)]
    pub resources: Option<serde_json::Value>,
    /// Whether the server supports prompt operations.
    #[serde(default)]
    pub prompts: Option<serde_json::Value>,
    /// Whether the server supports logging.
    #[serde(default)]
    pub logging: Option<serde_json::Value>,
}

impl ServerCapabilities {
    /// Returns `true` if the server advertises tool support.
    #[must_use]
    pub fn has_tools(&self) -> bool {
        self.tools.is_some()
    }

    /// Returns `true` if the server advertises resource support.
    #[must_use]
    pub fn has_resources(&self) -> bool {
        self.resources.is_some()
    }
}

/// Configuration for connecting to a specific MCP server.
///
/// This is the runtime configuration used by [`McpClient`](crate::McpClient),
/// distinct from the database-stored [`McpServerConfig`](sober_core::types::McpServerConfig).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerRunConfig {
    /// Display name for the server.
    pub name: String,
    /// Command to start the server.
    pub command: String,
    /// Command-line arguments.
    pub args: Vec<String>,
    /// Environment variables.
    pub env: std::collections::HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_info_deserializes() {
        let json = r#"{
            "name": "web_search",
            "description": "Search the web",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }
        }"#;
        let info: McpToolInfo = serde_json::from_str(json).expect("deserialize");
        assert_eq!(info.name, "web_search");
        assert_eq!(info.description.as_deref(), Some("Search the web"));
        assert!(info.input_schema.is_object());
    }

    #[test]
    fn tool_info_without_optional_fields() {
        let json = r#"{"name": "ping"}"#;
        let info: McpToolInfo = serde_json::from_str(json).expect("deserialize");
        assert_eq!(info.name, "ping");
        assert!(info.description.is_none());
    }

    #[test]
    fn resource_info_deserializes() {
        let json = r#"{
            "uri": "file:///tmp/data.txt",
            "name": "data.txt",
            "description": "A data file",
            "mimeType": "text/plain"
        }"#;
        let info: McpResourceInfo = serde_json::from_str(json).expect("deserialize");
        assert_eq!(info.uri, "file:///tmp/data.txt");
        assert_eq!(info.name, "data.txt");
        assert_eq!(info.mime_type.as_deref(), Some("text/plain"));
    }

    #[test]
    fn resource_content_text() {
        let content = ResourceContent {
            uri: "file:///test.txt".to_owned(),
            mime_type: Some("text/plain".to_owned()),
            text: Some("hello world".to_owned()),
            blob: None,
        };
        assert!(content.decode_blob().expect("no error").is_none());
    }

    #[test]
    fn resource_content_blob_decodes() {
        let data = b"binary data here";
        let encoded = BASE64_STANDARD.encode(data);
        let content = ResourceContent {
            uri: "file:///test.bin".to_owned(),
            mime_type: Some("application/octet-stream".to_owned()),
            text: None,
            blob: Some(encoded),
        };
        let decoded = content
            .decode_blob()
            .expect("no error")
            .expect("should have blob");
        assert_eq!(decoded, data);
    }

    #[test]
    fn resource_content_invalid_blob_returns_error() {
        let content = ResourceContent {
            uri: "file:///test.bin".to_owned(),
            mime_type: None,
            text: None,
            blob: Some("not-valid-base64!!!".to_owned()),
        };
        assert!(content.decode_blob().is_err());
    }

    #[test]
    fn tool_call_result_deserializes() {
        let json = r#"{
            "content": [
                {"type": "text", "text": "Search result: found 3 items"}
            ],
            "isError": false
        }"#;
        let result: ToolCallResult = serde_json::from_str(json).expect("deserialize");
        assert_eq!(result.content.len(), 1);
        assert!(!result.is_error);
        match &result.content[0] {
            ToolCallContent::Text { text } => assert!(text.contains("found 3 items")),
            _ => panic!("expected text content"),
        }
    }

    #[test]
    fn tool_call_result_with_error() {
        let json = r#"{
            "content": [
                {"type": "text", "text": "Error: permission denied"}
            ],
            "isError": true
        }"#;
        let result: ToolCallResult = serde_json::from_str(json).expect("deserialize");
        assert!(result.is_error);
    }

    #[test]
    fn server_info_deserializes() {
        let json = r#"{"name": "test-server", "version": "1.0.0"}"#;
        let info: ServerInfo = serde_json::from_str(json).expect("deserialize");
        assert_eq!(info.name, "test-server");
        assert_eq!(info.version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn server_capabilities_checks() {
        let caps = ServerCapabilities {
            tools: Some(serde_json::json!({})),
            resources: None,
            prompts: None,
            logging: None,
        };
        assert!(caps.has_tools());
        assert!(!caps.has_resources());
    }

    #[test]
    fn tool_call_content_image() {
        let json = r#"{
            "type": "image",
            "data": "aGVsbG8=",
            "mimeType": "image/png"
        }"#;
        let content: ToolCallContent = serde_json::from_str(json).expect("deserialize");
        match content {
            ToolCallContent::Image { data, mime_type } => {
                assert_eq!(data, "aGVsbG8=");
                assert_eq!(mime_type, "image/png");
            }
            _ => panic!("expected image content"),
        }
    }
}
