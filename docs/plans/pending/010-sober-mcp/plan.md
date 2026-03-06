# sober-mcp Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build the MCP client library that connects to local MCP servers over stdio, discovers tools + resources, and wraps them for use by sober-agent.

**Architecture:** Standalone crate (`sober-mcp`) with no dependency on `sober-agent` or `sober-api`. Spawns MCP server processes via `sober-sandbox`, communicates over JSON-RPC 2.0 / stdio. Connection pool manages per-user server lifecycles with crash recovery.

**Tech Stack:** Rust, tokio (process + io-util), serde/serde_json, thiserror, tracing. Depends on `sober-core` (types) and `sober-sandbox` (process isolation).

**Design doc:** `docs/plans/pending/010-sober-mcp/design.md`

---

## Prerequisites

- `sober-core` (003) must be implemented: `AppError`, `McpServerId`, config types.
- `sober-sandbox` (008) must be implemented. **Important:** the current sandbox design only has `BwrapSandbox::execute` (run-to-completion). MCP needs a `BwrapSandbox::spawn` method that returns a `Child` with captured stdin/stdout for long-running processes. This must be added to sober-sandbox before starting Task 5.

---

## Task 1: Scaffold sober-mcp crate

**Files:**
- Create: `backend/crates/sober-mcp/Cargo.toml`
- Create: `backend/crates/sober-mcp/src/lib.rs`
- Modify: `backend/Cargo.toml` (add `sober-mcp` to workspace members)

**Step 1: Create the crate directory**

```bash
mkdir -p backend/crates/sober-mcp/src
```

**Step 2: Create Cargo.toml**

```toml
[package]
name = "sober-mcp"
version = "0.1.0"
edition = "2024"

[dependencies]
sober-core = { path = "../sober-core" }
sober-sandbox = { path = "../sober-sandbox" }
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["process", "io-util", "sync", "time", "macros"] }
tracing = "0.1"

[dev-dependencies]
tokio = { version = "1", features = ["full", "test-util"] }
```

**Step 3: Create lib.rs with module declarations**

```rust
pub mod config;
pub mod error;
mod jsonrpc;
pub mod types;
pub mod client;
pub mod adapter;
pub mod pool;
```

**Step 4: Add to workspace**

Add `"crates/sober-mcp"` to the `members` list in `backend/Cargo.toml`.

**Step 5: Verify it compiles**

Run: `cd backend && cargo check -p sober-mcp`
Expected: Compilation errors (modules don't exist yet). That's fine --- scaffold is in place.

**Step 6: Commit**

```bash
git add backend/crates/sober-mcp backend/Cargo.toml
git commit -m "feat(mcp): scaffold sober-mcp crate"
```

---

## Task 2: Error types and config

**Files:**
- Create: `backend/crates/sober-mcp/src/error.rs`
- Create: `backend/crates/sober-mcp/src/config.rs`

**Step 1: Write the test for McpError -> AppError conversion**

Create `backend/crates/sober-mcp/src/error.rs`:

```rust
use sober_core::error::AppError;
use sober_sandbox::SandboxError;

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("initialize failed: {0}")]
    InitializeFailed(String),

    #[error("tool call failed: {0}")]
    ToolCallFailed(String),

    #[error("resource read failed: {0}")]
    ResourceReadFailed(String),

    #[error("protocol error: {0}")]
    ProtocolError(String),

    #[error("server unavailable: {0}")]
    ServerUnavailable(String),

    #[error("timeout after {0}s")]
    Timeout(u32),

    #[error("sandbox error: {0}")]
    Sandbox(#[from] SandboxError),
}

impl From<McpError> for AppError {
    fn from(err: McpError) -> Self {
        AppError::Internal(err.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_error_converts_to_app_error() {
        let mcp_err = McpError::ConnectionFailed("test".into());
        let app_err: AppError = mcp_err.into();
        assert!(matches!(app_err, AppError::Internal(_)));
    }

    #[test]
    fn timeout_displays_seconds() {
        let err = McpError::Timeout(30);
        assert_eq!(err.to_string(), "timeout after 30s");
    }
}
```

**Step 2: Create config.rs**

```rust
/// Configuration for MCP client behavior.
pub struct McpConfig {
    /// Timeout for individual MCP requests in seconds.
    pub request_timeout_secs: u32,

    /// Consecutive failures before marking a server unavailable.
    pub max_consecutive_failures: u32,

    /// Seconds of idle time before disconnecting an MCP server.
    pub idle_timeout_secs: u32,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            request_timeout_secs: 30,
            max_consecutive_failures: 3,
            idle_timeout_secs: 300,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let config = McpConfig::default();
        assert_eq!(config.request_timeout_secs, 30);
        assert_eq!(config.max_consecutive_failures, 3);
        assert_eq!(config.idle_timeout_secs, 300);
    }
}
```

**Step 3: Run tests**

Run: `cd backend && cargo test -p sober-mcp`
Expected: All tests pass.

**Step 4: Commit**

```bash
git add backend/crates/sober-mcp/src/error.rs backend/crates/sober-mcp/src/config.rs
git commit -m "feat(mcp): add error types and config"
```

---

## Task 3: JSON-RPC types

**Files:**
- Create: `backend/crates/sober-mcp/src/jsonrpc.rs`

These are internal types (not `pub` from the crate root) for JSON-RPC 2.0 framing.

**Step 1: Write tests first**

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub(crate) struct JsonRpcRequest {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 notification (no id, no response expected).
#[derive(Debug, Serialize)]
pub(crate) struct JsonRpcNotification {
    pub jsonrpc: &'static str,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct JsonRpcResponse {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: u64,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[allow(dead_code)]
    pub data: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes_correctly() {
        let req = JsonRpcRequest::new(1, "initialize", Some(serde_json::json!({"key": "val"})));
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 1);
        assert_eq!(parsed["method"], "initialize");
        assert_eq!(parsed["params"]["key"], "val");
    }

    #[test]
    fn request_omits_null_params() {
        let req = JsonRpcRequest::new(1, "tools/list", None);
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("params"));
    }

    #[test]
    fn notification_has_no_id() {
        let notif = JsonRpcNotification::new("notifications/initialized", None);
        let json = serde_json::to_string(&notif).unwrap();
        assert!(!json.contains("\"id\""));
    }

    #[test]
    fn response_deserializes_success() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"name":"test"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, 1);
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn response_deserializes_error() {
        let json = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"Invalid Request"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32600);
        assert_eq!(err.message, "Invalid Request");
    }
}
```

**Step 2: Run tests**

Run: `cd backend && cargo test -p sober-mcp -- jsonrpc`
Expected: All 5 tests pass.

**Step 3: Commit**

```bash
git add backend/crates/sober-mcp/src/jsonrpc.rs
git commit -m "feat(mcp): add JSON-RPC 2.0 types"
```

---

## Task 4: MCP domain types

**Files:**
- Create: `backend/crates/sober-mcp/src/types.rs`

**Step 1: Write types and tests**

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Tool information discovered from an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Resource information discovered from an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceInfo {
    pub uri: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,
}

/// Content returned when reading a resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContent {
    pub uri: String,
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,
    pub text: Option<String>,
    #[serde(with = "optional_base64", default)]
    pub blob: Option<Vec<u8>>,
}

mod optional_base64 {
    use serde::{Deserialize, Deserializer, Serializer};
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;

    pub fn serialize<S>(value: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(bytes) => serializer.serialize_some(&STANDARD.encode(bytes)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => STANDARD.decode(&s)
                .map(Some)
                .map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

/// Result of calling a tool.
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub content: String,
    pub is_error: bool,
}

/// Information about the MCP server, received during initialize handshake.
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub capabilities: ServerCapabilities,
}

/// Capabilities the server declares during initialize.
#[derive(Debug, Clone, Default)]
pub struct ServerCapabilities {
    pub tools: bool,
    pub resources: bool,
    pub prompts: bool,
}

/// Configuration for a user's MCP server (mirrors DB row).
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub id: sober_core::McpServerId,
    pub command: String,
    pub args: Vec<String>,
    pub env: std::collections::HashMap<String, String>,
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_info_deserializes_from_mcp_response() {
        let json = r#"{
            "name": "read_file",
            "description": "Read a file from disk",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }
        }"#;
        let info: McpToolInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.name, "read_file");
        assert_eq!(info.description, "Read a file from disk");
        assert!(info.input_schema.is_object());
    }

    #[test]
    fn tool_info_handles_missing_description() {
        let json = r#"{"name": "test", "inputSchema": {}}"#;
        let info: McpToolInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.description, "");
    }

    #[test]
    fn resource_info_deserializes() {
        let json = r#"{
            "uri": "postgres://tables/users",
            "name": "users table",
            "description": "Schema for users table",
            "mimeType": "application/json"
        }"#;
        let info: McpResourceInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.uri, "postgres://tables/users");
        assert_eq!(info.mime_type, Some("application/json".into()));
    }

    #[test]
    fn resource_content_text() {
        let json = r#"{
            "uri": "file:///test.txt",
            "mimeType": "text/plain",
            "text": "hello world"
        }"#;
        let content: ResourceContent = serde_json::from_str(json).unwrap();
        assert_eq!(content.text, Some("hello world".into()));
        assert!(content.blob.is_none());
    }

    #[test]
    fn server_capabilities_default_all_false() {
        let caps = ServerCapabilities::default();
        assert!(!caps.tools);
        assert!(!caps.resources);
        assert!(!caps.prompts);
    }
}
```

**Note:** Add `base64` to `[dependencies]` in Cargo.toml:

```toml
base64 = "0.22"
```

**Step 2: Run tests**

Run: `cd backend && cargo test -p sober-mcp -- types`
Expected: All 5 tests pass.

**Step 3: Commit**

```bash
git add backend/crates/sober-mcp/src/types.rs backend/crates/sober-mcp/Cargo.toml
git commit -m "feat(mcp): add MCP domain types"
```

---

## Task 5: McpClient — core JSON-RPC transport

**Files:**
- Create: `backend/crates/sober-mcp/src/client.rs`

This is the largest task. The client handles stdio I/O, the MCP initialize handshake,
and all protocol methods.

**Prerequisite:** `sober-sandbox` must expose a `BwrapSandbox::spawn` method that
starts a long-running sandboxed process and returns a `Child` with captured
stdin/stdout. If this method does not exist yet, add it to `sober-sandbox` first:

```rust
// In sober-sandbox, add to BwrapSandbox:
pub async fn spawn(
    &self,
    command: &[String],
    env: &HashMap<String, String>,
) -> Result<Child, SandboxError>;
```

This method builds the same bwrap argument list as `execute` but calls
`tokio::process::Command::spawn()` instead of `.output()`, returning
the `Child` handle with stdin/stdout piped.

**Step 1: Implement McpClient**

```rust
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::time::{timeout, Duration};
use tracing::{debug, warn, instrument};

use crate::config::McpConfig;
use crate::error::McpError;
use crate::jsonrpc::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use crate::types::*;
use sober_sandbox::BwrapSandbox;

pub struct McpClient {
    process: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    server_info: ServerInfo,
    request_id: AtomicU64,
    config: McpConfig,
}

impl McpClient {
    /// Spawn an MCP server in a sandbox and perform the initialize handshake.
    #[instrument(skip(sandbox, env, config), fields(command))]
    pub async fn connect(
        sandbox: &BwrapSandbox,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
        config: McpConfig,
    ) -> Result<Self, McpError> {
        let mut cmd_args = vec![command.to_string()];
        cmd_args.extend(args.iter().cloned());

        let mut child = sandbox
            .spawn(&cmd_args, env)
            .await
            .map_err(McpError::from)?;

        let child_stdin = child.stdin.take()
            .ok_or_else(|| McpError::ConnectionFailed("failed to capture stdin".into()))?;
        let child_stdout = child.stdout.take()
            .ok_or_else(|| McpError::ConnectionFailed("failed to capture stdout".into()))?;

        let mut client = Self {
            process: child,
            stdin: BufWriter::new(child_stdin),
            stdout: BufReader::new(child_stdout),
            server_info: ServerInfo {
                name: String::new(),
                version: String::new(),
                capabilities: ServerCapabilities::default(),
            },
            request_id: AtomicU64::new(1),
            config,
        };

        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&mut self) -> Result<(), McpError> {
        let params = serde_json::json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {
                "name": "sober-mcp",
                "version": env!("CARGO_PKG_VERSION")
            }
        });

        let response = self.send_request("initialize", Some(params)).await
            .map_err(|e| McpError::InitializeFailed(e.to_string()))?;

        // Parse server info from response
        let result = response.result
            .ok_or_else(|| McpError::InitializeFailed("no result in response".into()))?;

        self.server_info.name = result["serverInfo"]["name"]
            .as_str().unwrap_or("unknown").to_string();
        self.server_info.version = result["serverInfo"]["version"]
            .as_str().unwrap_or("0.0.0").to_string();

        if let Some(caps) = result.get("capabilities") {
            self.server_info.capabilities.tools = caps.get("tools").is_some();
            self.server_info.capabilities.resources = caps.get("resources").is_some();
            self.server_info.capabilities.prompts = caps.get("prompts").is_some();
        }

        // Send initialized notification (no response expected)
        self.send_notification("notifications/initialized", None).await?;

        debug!(
            server = %self.server_info.name,
            version = %self.server_info.version,
            tools = self.server_info.capabilities.tools,
            resources = self.server_info.capabilities.resources,
            "MCP server initialized"
        );

        Ok(())
    }

    /// List tools available on the server.
    pub async fn list_tools(&mut self) -> Result<Vec<McpToolInfo>, McpError> {
        if !self.server_info.capabilities.tools {
            return Ok(vec![]);
        }
        let response = self.send_request("tools/list", None).await?;
        let result = response.result.unwrap_or_default();
        let tools: Vec<McpToolInfo> = serde_json::from_value(
            result.get("tools").cloned().unwrap_or(serde_json::Value::Array(vec![]))
        ).map_err(|e| McpError::ProtocolError(format!("failed to parse tools: {e}")))?;
        Ok(tools)
    }

    /// Call a tool by name with the given input.
    pub async fn call_tool(
        &mut self,
        name: &str,
        input: serde_json::Value,
    ) -> Result<ToolCallResult, McpError> {
        let params = serde_json::json!({
            "name": name,
            "arguments": input,
        });
        let response = self.send_request("tools/call", Some(params)).await?;
        let result = response.result
            .ok_or_else(|| McpError::ToolCallFailed(format!("no result for tool {name}")))?;

        let is_error = result.get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let content = result.get("content")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        Ok(ToolCallResult { content, is_error })
    }

    /// List resources available on the server.
    pub async fn list_resources(&mut self) -> Result<Vec<McpResourceInfo>, McpError> {
        if !self.server_info.capabilities.resources {
            return Ok(vec![]);
        }
        let response = self.send_request("resources/list", None).await?;
        let result = response.result.unwrap_or_default();
        let resources: Vec<McpResourceInfo> = serde_json::from_value(
            result.get("resources").cloned().unwrap_or(serde_json::Value::Array(vec![]))
        ).map_err(|e| McpError::ProtocolError(format!("failed to parse resources: {e}")))?;
        Ok(resources)
    }

    /// Read a resource by URI.
    pub async fn read_resource(&mut self, uri: &str) -> Result<ResourceContent, McpError> {
        let params = serde_json::json!({ "uri": uri });
        let response = self.send_request("resources/read", Some(params)).await?;
        let result = response.result
            .ok_or_else(|| McpError::ResourceReadFailed(format!("no result for {uri}")))?;

        let contents = result.get("contents")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| McpError::ResourceReadFailed("empty contents".into()))?;

        serde_json::from_value(contents.clone())
            .map_err(|e| McpError::ProtocolError(format!("failed to parse resource: {e}")))
    }

    /// Gracefully shut down the MCP server.
    pub async fn shutdown(mut self) -> Result<(), McpError> {
        // Best effort --- ignore errors during shutdown
        let _ = self.send_notification("notifications/cancelled", None).await;
        drop(self.stdin);
        let _ = self.process.wait().await;
        Ok(())
    }

    /// Returns the server info from the initialize handshake.
    pub fn server_info(&self) -> &ServerInfo {
        &self.server_info
    }

    /// Check if the server process is still alive.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.process.try_wait(), Ok(None))
    }

    // --- Internal transport ---

    async fn send_request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<JsonRpcResponse, McpError> {
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);
        let request = JsonRpcRequest::new(id, method, params);

        let mut line = serde_json::to_string(&request)
            .map_err(|e| McpError::ProtocolError(format!("serialize failed: {e}")))?;
        line.push('\n');

        let timeout_duration = Duration::from_secs(self.config.request_timeout_secs as u64);

        timeout(timeout_duration, async {
            self.stdin.write_all(line.as_bytes()).await
                .map_err(|e| McpError::ConnectionFailed(format!("write failed: {e}")))?;
            self.stdin.flush().await
                .map_err(|e| McpError::ConnectionFailed(format!("flush failed: {e}")))?;

            self.read_response(id).await
        })
        .await
        .map_err(|_| McpError::Timeout(self.config.request_timeout_secs))?
    }

    async fn send_notification(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), McpError> {
        let notification = JsonRpcNotification::new(method, params);
        let mut line = serde_json::to_string(&notification)
            .map_err(|e| McpError::ProtocolError(format!("serialize failed: {e}")))?;
        line.push('\n');

        self.stdin.write_all(line.as_bytes()).await
            .map_err(|e| McpError::ConnectionFailed(format!("write failed: {e}")))?;
        self.stdin.flush().await
            .map_err(|e| McpError::ConnectionFailed(format!("flush failed: {e}")))?;

        Ok(())
    }

    async fn read_response(&mut self, expected_id: u64) -> Result<JsonRpcResponse, McpError> {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = self.stdout.read_line(&mut line).await
                .map_err(|e| McpError::ConnectionFailed(format!("read failed: {e}")))?;

            if bytes_read == 0 {
                return Err(McpError::ConnectionFailed("server closed stdout (EOF)".into()));
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Try to parse as a response. Skip notifications from server.
            if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                if response.id == expected_id {
                    if let Some(ref err) = response.error {
                        return Err(McpError::ProtocolError(
                            format!("server error {}: {}", err.code, err.message)
                        ));
                    }
                    return Ok(response);
                }
                warn!(
                    expected = expected_id,
                    got = response.id,
                    "received response with unexpected id, skipping"
                );
            }
            // Not a response (likely a notification from server) --- skip
        }
    }
}
```

**Step 2: Run cargo check**

Run: `cd backend && cargo check -p sober-mcp`
Expected: Compiles (assuming sober-core and sober-sandbox are available).

**Step 3: Commit**

```bash
git add backend/crates/sober-mcp/src/client.rs
git commit -m "feat(mcp): implement McpClient with JSON-RPC transport"
```

---

## Task 6: Adapters (McpToolAdapter + McpResourceAdapter)

**Files:**
- Create: `backend/crates/sober-mcp/src/adapter.rs`

**Note:** The `Tool` trait is defined in `sober-agent`. Since `sober-mcp` must not
depend on `sober-agent`, the `McpToolAdapter` provides the data needed for the agent
to construct tool definitions, but does not implement the agent's `Tool` trait directly.
Instead, it exposes methods that the agent's adapter layer can delegate to.

Alternatively, the `Tool` trait could live in `sober-core` so both crates can
use it. The implementor should check where `Tool` ends up and adjust accordingly.
The adapter code below assumes the `Tool` and `Resource` traits are importable.

**Step 1: Write adapter code and tests**

```rust
use std::sync::Arc;
use tokio::sync::Mutex;
use async_trait::async_trait;

use crate::client::McpClient;
use crate::error::McpError;
use crate::types::{McpToolInfo, McpResourceInfo, ResourceContent, ToolCallResult};

/// Wraps an MCP tool, proxying calls to the McpClient.
pub struct McpToolAdapter {
    client: Arc<Mutex<McpClient>>,
    tool_info: McpToolInfo,
    server_name: String,
}

impl McpToolAdapter {
    pub fn new(
        client: Arc<Mutex<McpClient>>,
        tool_info: McpToolInfo,
        server_name: String,
    ) -> Self {
        Self { client, tool_info, server_name }
    }

    pub fn name(&self) -> &str {
        &self.tool_info.name
    }

    pub fn description(&self) -> &str {
        &self.tool_info.description
    }

    pub fn input_schema(&self) -> &serde_json::Value {
        &self.tool_info.input_schema
    }

    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    pub async fn call(&self, input: serde_json::Value) -> Result<ToolCallResult, McpError> {
        let mut client = self.client.lock().await;
        client.call_tool(&self.tool_info.name, input).await
    }
}

/// Wraps an MCP resource, proxying reads to the McpClient.
#[async_trait]
pub trait Resource: Send + Sync {
    fn uri(&self) -> &str;
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn mime_type(&self) -> Option<&str>;
    async fn read(&self) -> Result<ResourceContent, McpError>;
}

pub struct McpResourceAdapter {
    client: Arc<Mutex<McpClient>>,
    resource_info: McpResourceInfo,
    server_name: String,
}

impl McpResourceAdapter {
    pub fn new(
        client: Arc<Mutex<McpClient>>,
        resource_info: McpResourceInfo,
        server_name: String,
    ) -> Self {
        Self { client, resource_info, server_name }
    }

    pub fn server_name(&self) -> &str {
        &self.server_name
    }
}

#[async_trait]
impl Resource for McpResourceAdapter {
    fn uri(&self) -> &str {
        &self.resource_info.uri
    }

    fn name(&self) -> &str {
        &self.resource_info.name
    }

    fn description(&self) -> &str {
        &self.resource_info.description
    }

    fn mime_type(&self) -> Option<&str> {
        self.resource_info.mime_type.as_deref()
    }

    async fn read(&self) -> Result<ResourceContent, McpError> {
        let mut client = self.client.lock().await;
        client.read_resource(&self.resource_info.uri).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_adapter_exposes_info() {
        let info = McpToolInfo {
            name: "read_file".into(),
            description: "Read a file".into(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        // Can't construct McpClient in tests without a process,
        // so just test the info accessors conceptually.
        assert_eq!(info.name, "read_file");
        assert_eq!(info.description, "Read a file");
    }

    #[test]
    fn resource_info_mime_type() {
        let info = McpResourceInfo {
            uri: "file:///test".into(),
            name: "test".into(),
            description: "test resource".into(),
            mime_type: Some("text/plain".into()),
        };
        assert_eq!(info.mime_type.as_deref(), Some("text/plain"));
    }
}
```

**Step 2: Run tests**

Run: `cd backend && cargo test -p sober-mcp -- adapter`
Expected: Tests pass.

**Step 3: Commit**

```bash
git add backend/crates/sober-mcp/src/adapter.rs
git commit -m "feat(mcp): add tool and resource adapters"
```

---

## Task 7: Connection pool with crash recovery

**Files:**
- Create: `backend/crates/sober-mcp/src/pool.rs`

**Step 1: Implement McpPool**

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn, error, instrument};

use crate::adapter::{McpToolAdapter, McpResourceAdapter};
use crate::client::McpClient;
use crate::config::McpConfig;
use crate::error::McpError;
use crate::types::McpServerConfig;
use sober_core::McpServerId;
use sober_sandbox::BwrapSandbox;

/// Discovery result: all tools and resources from connected MCP servers.
pub struct McpDiscovery {
    pub tools: Vec<McpToolAdapter>,
    pub resources: Vec<McpResourceAdapter>,
}

/// Tracks per-server failure state for crash recovery.
struct ServerState {
    client: Arc<Mutex<McpClient>>,
    config: McpServerConfig,
    consecutive_failures: u32,
    unavailable: bool,
}

/// Connection pool managing MCP server lifecycles for a single user.
pub struct McpPool {
    servers: RwLock<HashMap<McpServerId, ServerState>>,
    sandbox: Arc<BwrapSandbox>,
    config: McpConfig,
}

impl McpPool {
    pub fn new(sandbox: Arc<BwrapSandbox>, config: McpConfig) -> Self {
        Self {
            servers: RwLock::new(HashMap::new()),
            sandbox,
            config,
        }
    }

    /// Connect to all enabled MCP servers for a user.
    #[instrument(skip(self, server_configs))]
    pub async fn connect_user_servers(
        &self,
        server_configs: &[McpServerConfig],
    ) -> Result<(), McpError> {
        let mut servers = self.servers.write().await;
        for cfg in server_configs.iter().filter(|c| c.enabled) {
            match McpClient::connect(
                &self.sandbox,
                &cfg.command,
                &cfg.args,
                &cfg.env,
                McpConfig { ..McpConfig::default() },
            ).await {
                Ok(client) => {
                    info!(server = %cfg.command, id = %cfg.id, "MCP server connected");
                    servers.insert(cfg.id, ServerState {
                        client: Arc::new(Mutex::new(client)),
                        config: cfg.clone(),
                        consecutive_failures: 0,
                        unavailable: false,
                    });
                }
                Err(e) => {
                    warn!(server = %cfg.command, error = %e, "failed to connect MCP server, skipping");
                }
            }
        }
        Ok(())
    }

    /// Discover all tools and resources from connected servers.
    pub async fn discover(&self) -> Result<McpDiscovery, McpError> {
        let servers = self.servers.read().await;
        let mut tools = Vec::new();
        let mut resources = Vec::new();

        for (_, state) in servers.iter() {
            if state.unavailable {
                continue;
            }
            let mut client = state.client.lock().await;
            let server_name = client.server_info().name.clone();

            match client.list_tools().await {
                Ok(server_tools) => {
                    for tool_info in server_tools {
                        tools.push(McpToolAdapter::new(
                            Arc::clone(&state.client),
                            tool_info,
                            server_name.clone(),
                        ));
                    }
                }
                Err(e) => {
                    warn!(server = %server_name, error = %e, "failed to list tools");
                }
            }

            match client.list_resources().await {
                Ok(server_resources) => {
                    for resource_info in server_resources {
                        resources.push(McpResourceAdapter::new(
                            Arc::clone(&state.client),
                            resource_info,
                            server_name.clone(),
                        ));
                    }
                }
                Err(e) => {
                    warn!(server = %server_name, error = %e, "failed to list resources");
                }
            }
        }

        Ok(McpDiscovery { tools, resources })
    }

    /// Record a failure for a server. Returns true if the server is now unavailable.
    pub async fn record_failure(&self, server_id: McpServerId) -> bool {
        let mut servers = self.servers.write().await;
        if let Some(state) = servers.get_mut(&server_id) {
            state.consecutive_failures += 1;
            if state.consecutive_failures >= self.config.max_consecutive_failures {
                state.unavailable = true;
                error!(
                    server_id = %server_id,
                    failures = state.consecutive_failures,
                    "MCP server marked unavailable after consecutive failures"
                );
                return true;
            }
        }
        false
    }

    /// Reset failure count for a server (called on successful call).
    pub async fn record_success(&self, server_id: McpServerId) {
        let mut servers = self.servers.write().await;
        if let Some(state) = servers.get_mut(&server_id) {
            state.consecutive_failures = 0;
        }
    }

    /// Attempt to reconnect a specific server. Returns Ok if successful.
    pub async fn try_reconnect(&self, server_id: McpServerId) -> Result<(), McpError> {
        let mut servers = self.servers.write().await;
        let state = servers.get(&server_id)
            .ok_or_else(|| McpError::ConnectionFailed(format!("unknown server {server_id}")))?;

        let cfg = state.config.clone();
        drop(servers); // release lock before connecting

        let client = McpClient::connect(
            &self.sandbox,
            &cfg.command,
            &cfg.args,
            &cfg.env,
            McpConfig { ..McpConfig::default() },
        ).await?;

        let mut servers = self.servers.write().await;
        if let Some(state) = servers.get_mut(&server_id) {
            state.client = Arc::new(Mutex::new(client));
            state.consecutive_failures = 0;
            state.unavailable = false;
            info!(server_id = %server_id, "MCP server reconnected");
        }
        Ok(())
    }

    /// Shut down all connected MCP servers.
    pub async fn shutdown(&self) -> Result<(), McpError> {
        let mut servers = self.servers.write().await;
        for (id, state) in servers.drain() {
            let client = Arc::try_unwrap(state.client)
                .map_err(|_| McpError::ConnectionFailed(
                    format!("cannot shutdown server {id}: client still in use")
                ))?;
            let client = client.into_inner();
            if let Err(e) = client.shutdown().await {
                warn!(server_id = %id, error = %e, "error during MCP server shutdown");
            }
        }
        Ok(())
    }
}
```

**Step 2: Run cargo check**

Run: `cd backend && cargo check -p sober-mcp`
Expected: Compiles.

**Step 3: Commit**

```bash
git add backend/crates/sober-mcp/src/pool.rs
git commit -m "feat(mcp): add connection pool with crash recovery"
```

---

## Task 8: Wire up lib.rs re-exports

**Files:**
- Modify: `backend/crates/sober-mcp/src/lib.rs`

**Step 1: Update lib.rs with clean public API**

```rust
//! MCP client library for Sober.
//!
//! Connects to local MCP servers over stdio, discovers their tools and resources,
//! and wraps them for use by the agent layer.

pub mod adapter;
pub mod client;
pub mod config;
pub mod error;
mod jsonrpc;
pub mod pool;
pub mod types;

// Re-export primary types for convenience
pub use adapter::{McpResourceAdapter, McpToolAdapter, Resource};
pub use client::McpClient;
pub use config::McpConfig;
pub use error::McpError;
pub use pool::{McpDiscovery, McpPool};
pub use types::{
    McpResourceInfo, McpServerConfig, McpToolInfo, ResourceContent, ServerCapabilities,
    ServerInfo, ToolCallResult,
};
```

**Step 2: Run cargo check and clippy**

Run: `cd backend && cargo clippy -p sober-mcp -- -D warnings`
Expected: No warnings. Fix any that appear.

**Step 3: Commit**

```bash
git add backend/crates/sober-mcp/src/lib.rs
git commit -m "feat(mcp): wire up public API re-exports"
```

---

## Task 9: Integration test with mock MCP server

**Files:**
- Create: `backend/crates/sober-mcp/tests/integration.rs`
- Create: `backend/crates/sober-mcp/tests/mock_server.py` (or .sh)

A real integration test needs an MCP server process. The simplest approach is a
small Python or shell script that speaks JSON-RPC over stdio.

**Step 1: Create a mock MCP server**

Create `backend/crates/sober-mcp/tests/mock_mcp_server.py`:

```python
#!/usr/bin/env python3
"""Minimal MCP server for integration testing. Speaks JSON-RPC 2.0 over stdio."""
import json
import sys

def respond(id, result):
    msg = json.dumps({"jsonrpc": "2.0", "id": id, "result": result})
    sys.stdout.write(msg + "\n")
    sys.stdout.flush()

def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue

        # Notifications have no id
        if "id" not in msg:
            continue

        method = msg.get("method", "")
        msg_id = msg["id"]

        if method == "initialize":
            respond(msg_id, {
                "protocolVersion": "2025-03-26",
                "capabilities": {
                    "tools": {},
                    "resources": {}
                },
                "serverInfo": {
                    "name": "mock-mcp",
                    "version": "0.1.0"
                }
            })
        elif method == "tools/list":
            respond(msg_id, {
                "tools": [
                    {
                        "name": "echo",
                        "description": "Echoes back the input",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "message": {"type": "string"}
                            },
                            "required": ["message"]
                        }
                    }
                ]
            })
        elif method == "tools/call":
            args = msg.get("params", {}).get("arguments", {})
            message = args.get("message", "")
            respond(msg_id, {
                "content": [{"type": "text", "text": f"echo: {message}"}],
                "isError": False
            })
        elif method == "resources/list":
            respond(msg_id, {
                "resources": [
                    {
                        "uri": "test://hello",
                        "name": "hello",
                        "description": "A test resource",
                        "mimeType": "text/plain"
                    }
                ]
            })
        elif method == "resources/read":
            respond(msg_id, {
                "contents": [
                    {
                        "uri": "test://hello",
                        "mimeType": "text/plain",
                        "text": "Hello from mock MCP server!"
                    }
                ]
            })
        else:
            respond(msg_id, {})

if __name__ == "__main__":
    main()
```

Make it executable: `chmod +x backend/crates/sober-mcp/tests/mock_mcp_server.py`

**Step 2: Write integration test**

The integration test needs to bypass the sandbox (we're testing MCP protocol,
not sandboxing). Create a test helper that spawns the mock server directly
via `tokio::process::Command` and constructs an `McpClient` from the raw
process handle. This requires adding a `McpClient::from_process` test constructor.

Add to `client.rs`:

```rust
#[cfg(test)]
impl McpClient {
    /// Test-only constructor that takes a pre-spawned process.
    pub(crate) async fn from_process(
        mut child: Child,
        config: McpConfig,
    ) -> Result<Self, McpError> {
        let child_stdin = child.stdin.take()
            .ok_or_else(|| McpError::ConnectionFailed("no stdin".into()))?;
        let child_stdout = child.stdout.take()
            .ok_or_else(|| McpError::ConnectionFailed("no stdout".into()))?;

        let mut client = Self {
            process: child,
            stdin: BufWriter::new(child_stdin),
            stdout: BufReader::new(child_stdout),
            server_info: ServerInfo {
                name: String::new(),
                version: String::new(),
                capabilities: ServerCapabilities::default(),
            },
            request_id: AtomicU64::new(1),
            config,
        };

        client.initialize().await?;
        Ok(client)
    }
}
```

Create `backend/crates/sober-mcp/tests/integration.rs`:

```rust
use std::process::Stdio;
use sober_mcp::{McpConfig, McpClient};

async fn spawn_mock_server() -> McpClient {
    let child = tokio::process::Command::new("python3")
        .arg("tests/mock_mcp_server.py")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .spawn()
        .expect("failed to spawn mock MCP server (is python3 installed?)");

    McpClient::from_process(child, McpConfig::default())
        .await
        .expect("failed to initialize mock MCP server")
}

#[tokio::test]
async fn initialize_handshake() {
    let client = spawn_mock_server().await;
    let info = client.server_info();
    assert_eq!(info.name, "mock-mcp");
    assert_eq!(info.version, "0.1.0");
    assert!(info.capabilities.tools);
    assert!(info.capabilities.resources);
    assert!(!info.capabilities.prompts);
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn list_tools() {
    let mut client = spawn_mock_server().await;
    let tools = client.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn call_tool() {
    let mut client = spawn_mock_server().await;
    let result = client
        .call_tool("echo", serde_json::json!({"message": "hello"}))
        .await
        .unwrap();
    assert_eq!(result.content, "echo: hello");
    assert!(!result.is_error);
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn list_resources() {
    let mut client = spawn_mock_server().await;
    let resources = client.list_resources().await.unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].uri, "test://hello");
    assert_eq!(resources[0].mime_type, Some("text/plain".into()));
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn read_resource() {
    let mut client = spawn_mock_server().await;
    let content = client.read_resource("test://hello").await.unwrap();
    assert_eq!(content.text, Some("Hello from mock MCP server!".into()));
    assert_eq!(content.mime_type, Some("text/plain".into()));
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn full_lifecycle() {
    let mut client = spawn_mock_server().await;

    // List and call tools
    let tools = client.list_tools().await.unwrap();
    assert!(!tools.is_empty());
    let result = client
        .call_tool("echo", serde_json::json!({"message": "lifecycle test"}))
        .await
        .unwrap();
    assert!(result.content.contains("lifecycle test"));

    // List and read resources
    let resources = client.list_resources().await.unwrap();
    assert!(!resources.is_empty());
    let content = client.read_resource(&resources[0].uri).await.unwrap();
    assert!(content.text.is_some());

    // Shutdown
    client.shutdown().await.unwrap();
}
```

**Step 3: Run integration tests**

Run: `cd backend && cargo test -p sober-mcp --test integration`
Expected: All 6 tests pass.

**Step 4: Commit**

```bash
git add backend/crates/sober-mcp/tests/ backend/crates/sober-mcp/src/client.rs
git commit -m "test(mcp): add integration tests with mock MCP server"
```

---

## Task 10: Final clippy + docs pass

**Step 1: Run full check suite**

```bash
cd backend && cargo clippy -p sober-mcp -- -D warnings
cd backend && cargo test -p sober-mcp
cd backend && cargo doc -p sober-mcp --no-deps
```

Expected: No warnings, all tests pass, docs generate cleanly.

**Step 2: Fix any issues found**

**Step 3: Commit**

```bash
git commit -am "chore(mcp): clippy and doc fixes"
```

---

## Acceptance Criteria

- [ ] `sober-mcp` crate compiles independently (`cargo check -p sober-mcp`)
- [ ] `McpClient` can connect to a mock server, initialize, list tools, call a tool, and shutdown
- [ ] `McpClient` can list resources and read a resource
- [ ] `McpClient` respects configurable timeouts
- [ ] `McpToolAdapter` exposes tool info and proxies calls to `McpClient`
- [ ] `McpResourceAdapter` implements `Resource` trait and proxies reads
- [ ] `McpPool` connects to multiple servers, discovers tools + resources
- [ ] `McpPool` tracks consecutive failures and marks servers unavailable
- [ ] `McpPool` can attempt reconnection of failed servers
- [ ] `McpPool` shuts down all servers cleanly
- [ ] JSON-RPC serialization round-trips correctly
- [ ] `McpError` converts to `AppError::Internal`
- [ ] `McpConfig` has sensible defaults (30s timeout, 3 failures, 300s idle)
- [ ] `cargo clippy -p sober-mcp -- -D warnings` passes
- [ ] `cargo test -p sober-mcp` passes (unit + integration)
- [ ] No dependency on `sober-agent`, `sober-api`, or `sober-llm`
