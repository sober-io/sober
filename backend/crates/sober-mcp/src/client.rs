//! MCP client for communicating with a single MCP server over stdio.
//!
//! [`McpClient`] manages the child process, sends JSON-RPC requests, and
//! reads responses. It handles the MCP initialize handshake and provides
//! methods for tool and resource operations.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use metrics::{counter, histogram};
use sober_sandbox::{BwrapSandbox, SandboxPolicy};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::time::{Duration, timeout};
use tracing::{debug, warn};

use crate::config::McpConfig;
use crate::error::McpError;
use crate::jsonrpc::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use crate::types::{
    McpResourceInfo, McpServerRunConfig, McpToolInfo, ResourceContent, ServerCapabilities,
    ServerInfo, ToolCallResult,
};

/// The MCP protocol version this client implements.
const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

/// Client name sent during initialization.
const CLIENT_NAME: &str = "sober-mcp";

/// Client version sent during initialization.
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// MCP client managing a single server connection over stdio.
///
/// The client owns the child process and communicates via JSON-RPC 2.0
/// over stdin/stdout. It is not Clone; use `Arc<Mutex<McpClient>>` for
/// shared access.
pub struct McpClient {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    child: Child,
    next_id: AtomicU64,
    config: McpConfig,
    server_info: Option<ServerInfo>,
    capabilities: Option<ServerCapabilities>,
}

impl McpClient {
    /// Connect to an MCP server by spawning it inside a sandbox.
    ///
    /// This spawns the server process using bubblewrap and returns a client
    /// ready for the initialize handshake. Call [`initialize`](Self::initialize)
    /// after connecting.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Sandbox`] if sandbox spawning fails, or
    /// [`McpError::ConnectionFailed`] if stdin/stdout are not available.
    pub async fn connect(
        server_config: &McpServerRunConfig,
        sandbox_policy: SandboxPolicy,
        config: McpConfig,
    ) -> Result<Self, McpError> {
        let sandbox = BwrapSandbox::new(sandbox_policy);

        let mut command = vec![server_config.command.clone()];
        command.extend(server_config.args.iter().cloned());

        let child = sandbox.spawn(&command, &server_config.env).await?;

        Self::from_child(child, config)
    }

    /// Create a client from a pre-spawned child process.
    ///
    /// This bypasses the sandbox and is intended for testing. The child
    /// process must have piped stdin, stdout, and stderr.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::ConnectionFailed`] if stdin/stdout are not available.
    pub fn from_process(child: Child, config: McpConfig) -> Result<Self, McpError> {
        Self::from_child(child, config)
    }

    /// Internal constructor from a child process.
    fn from_child(mut child: Child, config: McpConfig) -> Result<Self, McpError> {
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::ConnectionFailed("child stdin not piped".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::ConnectionFailed("child stdout not piped".into()))?;

        Ok(Self {
            stdin,
            stdout: BufReader::new(stdout),
            child,
            next_id: AtomicU64::new(1),
            config,
            server_info: None,
            capabilities: None,
        })
    }

    /// Perform the MCP initialize handshake.
    ///
    /// Sends the `initialize` request and the `notifications/initialized`
    /// notification per the MCP spec. Must be called before any tool or
    /// resource operations.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::InitializeFailed`] on handshake failure or
    /// [`McpError::Timeout`] if the server doesn't respond in time.
    pub async fn initialize(&mut self) -> Result<(), McpError> {
        let params = serde_json::json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": CLIENT_NAME,
                "version": CLIENT_VERSION,
            }
        });

        let response = self.send_request("initialize", Some(params)).await?;

        let result = response.result.ok_or_else(|| {
            let err_msg = response
                .error
                .as_ref()
                .map_or("no result in initialize response".to_owned(), |e| {
                    format!("server error: {} (code {})", e.message, e.code)
                });
            McpError::InitializeFailed(err_msg)
        })?;

        // Parse server info.
        if let Some(info) = result.get("serverInfo") {
            self.server_info = serde_json::from_value(info.clone()).ok();
        }

        // Parse capabilities.
        if let Some(caps) = result.get("capabilities") {
            self.capabilities = serde_json::from_value(caps.clone()).ok();
        }

        // Send initialized notification.
        self.send_notification("notifications/initialized", None)
            .await?;

        debug!(
            server = ?self.server_info,
            capabilities = ?self.capabilities,
            "MCP initialize handshake complete"
        );

        Ok(())
    }

    /// List all tools provided by the MCP server.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::ToolCallFailed`] or [`McpError::ProtocolError`]
    /// on failure.
    pub async fn list_tools(&mut self) -> Result<Vec<McpToolInfo>, McpError> {
        let response = self.send_request("tools/list", None).await?;

        let result = Self::extract_result(response, "tools/list")?;

        let tools = result
            .get("tools")
            .cloned()
            .unwrap_or(serde_json::Value::Array(vec![]));

        serde_json::from_value(tools)
            .map_err(|e| McpError::ProtocolError(format!("failed to parse tools list: {e}")))
    }

    /// Call a tool on the MCP server.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::ToolCallFailed`] on failure or
    /// [`McpError::Timeout`] if the call exceeds the configured timeout.
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<ToolCallResult, McpError> {
        let start = Instant::now();
        let server = self
            .server_info
            .as_ref()
            .map_or("unknown", |s| s.name.as_str())
            .to_owned();

        let params = serde_json::json!({
            "name": name,
            "arguments": arguments,
        });

        let result = async {
            let response = self.send_request("tools/call", Some(params)).await?;
            let result = Self::extract_result(response, "tools/call")?;
            serde_json::from_value(result)
                .map_err(|e| McpError::ToolCallFailed(format!("failed to parse tool result: {e}")))
        }
        .await;

        let status = if result.is_ok() { "success" } else { "error" };
        let tool_name = name.to_owned();
        counter!(
            "sober_mcp_tool_calls_total",
            "server" => server.clone(),
            "tool" => tool_name.clone(),
            "status" => status,
        )
        .increment(1);
        histogram!(
            "sober_mcp_tool_call_duration_seconds",
            "server" => server,
            "tool" => tool_name,
        )
        .record(start.elapsed().as_secs_f64());

        result
    }

    /// List all resources provided by the MCP server.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::ResourceReadFailed`] or [`McpError::ProtocolError`]
    /// on failure.
    pub async fn list_resources(&mut self) -> Result<Vec<McpResourceInfo>, McpError> {
        let response = self.send_request("resources/list", None).await?;

        let result = Self::extract_result(response, "resources/list")?;

        let resources = result
            .get("resources")
            .cloned()
            .unwrap_or(serde_json::Value::Array(vec![]));

        serde_json::from_value(resources)
            .map_err(|e| McpError::ProtocolError(format!("failed to parse resources list: {e}")))
    }

    /// Read a resource from the MCP server by URI.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::ResourceReadFailed`] on failure.
    pub async fn read_resource(&mut self, uri: &str) -> Result<Vec<ResourceContent>, McpError> {
        let params = serde_json::json!({
            "uri": uri,
        });

        let response = self.send_request("resources/read", Some(params)).await?;

        let result = Self::extract_result(response, "resources/read")?;

        let contents = result
            .get("contents")
            .cloned()
            .unwrap_or(serde_json::Value::Array(vec![]));

        serde_json::from_value(contents).map_err(|e| {
            McpError::ResourceReadFailed(format!("failed to parse resource contents: {e}"))
        })
    }

    /// Gracefully shut down the MCP server connection.
    ///
    /// Sends a shutdown notification and kills the child process.
    pub async fn shutdown(&mut self) {
        // Best-effort shutdown notification.
        let _ = self
            .send_notification("notifications/cancelled", None)
            .await;

        // Kill the child process.
        let _ = self.child.kill().await;
    }

    /// Check if the server process is still running.
    #[must_use]
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Returns the server info from the initialize handshake, if available.
    #[must_use]
    pub fn server_info(&self) -> Option<&ServerInfo> {
        self.server_info.as_ref()
    }

    /// Returns the server capabilities from the initialize handshake, if available.
    #[must_use]
    pub fn capabilities(&self) -> Option<&ServerCapabilities> {
        self.capabilities.as_ref()
    }

    /// Send a JSON-RPC request and wait for the response.
    async fn send_request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<JsonRpcResponse, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = JsonRpcRequest::new(id, method, params);

        let mut request_bytes =
            serde_json::to_vec(&request).map_err(|e| McpError::ProtocolError(e.to_string()))?;
        request_bytes.push(b'\n');

        debug!(method, id, "sending MCP request");

        self.stdin
            .write_all(&request_bytes)
            .await
            .map_err(|e| McpError::ConnectionFailed(format!("failed to write to stdin: {e}")))?;

        self.stdin
            .flush()
            .await
            .map_err(|e| McpError::ConnectionFailed(format!("failed to flush stdin: {e}")))?;

        self.read_response(id).await
    }

    /// Send a JSON-RPC notification (no response expected).
    async fn send_notification(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), McpError> {
        let notification = JsonRpcNotification::new(method, params);

        let mut notif_bytes = serde_json::to_vec(&notification)
            .map_err(|e| McpError::ProtocolError(e.to_string()))?;
        notif_bytes.push(b'\n');

        self.stdin
            .write_all(&notif_bytes)
            .await
            .map_err(|e| McpError::ConnectionFailed(format!("failed to write to stdin: {e}")))?;

        self.stdin
            .flush()
            .await
            .map_err(|e| McpError::ConnectionFailed(format!("failed to flush stdin: {e}")))?;

        Ok(())
    }

    /// Read a JSON-RPC response with the given id, skipping notifications.
    async fn read_response(&mut self, expected_id: u64) -> Result<JsonRpcResponse, McpError> {
        let timeout_duration = Duration::from_secs(self.config.request_timeout_secs);

        timeout(timeout_duration, self.read_response_inner(expected_id))
            .await
            .map_err(|_| McpError::Timeout {
                seconds: self.config.request_timeout_secs,
            })?
    }

    /// Inner loop to read lines until we find the response with our id.
    async fn read_response_inner(&mut self, expected_id: u64) -> Result<JsonRpcResponse, McpError> {
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read =
                self.stdout.read_line(&mut line).await.map_err(|e| {
                    McpError::ConnectionFailed(format!("failed to read stdout: {e}"))
                })?;

            if bytes_read == 0 {
                return Err(McpError::ConnectionFailed(
                    "server closed stdout (EOF)".into(),
                ));
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Try to parse as a response.
            let parsed: serde_json::Value = serde_json::from_str(trimmed)
                .map_err(|e| McpError::ProtocolError(format!("invalid JSON from server: {e}")))?;

            // Skip notifications (messages without an id).
            if parsed.get("id").is_none() {
                debug!(
                    method = parsed
                        .get("method")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown"),
                    "received server notification, skipping"
                );
                continue;
            }

            let response: JsonRpcResponse = serde_json::from_value(parsed).map_err(|e| {
                McpError::ProtocolError(format!("failed to parse JSON-RPC response: {e}"))
            })?;

            if response.id == Some(expected_id) {
                return Ok(response);
            }

            // Mismatched id -- log and continue.
            warn!(
                expected = expected_id,
                received = ?response.id,
                "received response with unexpected id"
            );
        }
    }

    /// Extract the result from a JSON-RPC response, mapping errors.
    fn extract_result(
        response: JsonRpcResponse,
        method: &str,
    ) -> Result<serde_json::Value, McpError> {
        if let Some(err) = response.error {
            return Err(McpError::ToolCallFailed(format!(
                "{method}: {} (code {})",
                err.message, err.code
            )));
        }

        response
            .result
            .ok_or_else(|| McpError::ProtocolError(format!("{method}: no result in response")))
    }
}

// We need to allow dead_code for the `_env` field in tests since
// McpServerRunConfig is used but HashMap may not be fully consumed.
#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn mcp_protocol_version_is_correct() {
        assert_eq!(MCP_PROTOCOL_VERSION, "2025-03-26");
    }

    #[test]
    fn client_name_and_version() {
        assert_eq!(CLIENT_NAME, "sober-mcp");
        assert!(!CLIENT_VERSION.is_empty());
    }

    #[test]
    fn server_run_config_roundtrip() {
        let config = McpServerRunConfig {
            name: "test".into(),
            command: "python3".into(),
            args: vec!["-m".into(), "mcp_server".into()],
            env: HashMap::from([("API_KEY".into(), "secret".into())]),
        };
        let json = serde_json::to_value(&config).expect("serialize");
        assert_eq!(json["command"], "python3");
        assert_eq!(json["args"][0], "-m");
    }
}
