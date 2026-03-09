# 011 --- sober-mcp

**Date:** 2026-03-06

---

## Scope & Role

`sober-mcp` is a standalone MCP **client** library. It connects to local MCP
servers over **stdio**, discovers and invokes their **tools** and **resources**,
and wraps them for use by `sober-agent`.

- **Protocol:** MCP spec `2025-03-26`
- **Role:** Client only (server role deferred)
- **Transport:** stdio only (local servers)
- **Capabilities:** Tools + Resources (Prompts and Sampling deferred)
- **Auth:** None (local processes only)
- **Sandboxing:** MCP server processes spawned through `sober-sandbox` (bwrap)

---

## MCP Client

```rust
pub struct McpClient {
    process: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    server_info: ServerInfo,
    request_id: AtomicU64,
}
```

### API

```rust
impl McpClient {
    /// Spawn MCP server via sober-sandbox, perform initialize handshake.
    pub async fn connect(
        sandbox: &BwrapSandbox,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self, McpError>;

    pub async fn list_tools(&mut self) -> Result<Vec<McpToolInfo>, McpError>;
    pub async fn call_tool(&mut self, name: &str, input: Value) -> Result<ToolCallResult, McpError>;
    pub async fn list_resources(&mut self) -> Result<Vec<McpResourceInfo>, McpError>;
    pub async fn read_resource(&mut self, uri: &str) -> Result<ResourceContent, McpError>;
    pub async fn shutdown(self) -> Result<(), McpError>;
}
```

### Protocol details

- JSON-RPC 2.0 over stdin/stdout, one JSON object per line (newline-delimited)
- `initialize` handshake: client declares capabilities, server responds with its
  capabilities and server info
- Client checks `server_info.capabilities` before calling `tools/list` or
  `resources/list` --- only requests what the server supports
- Request timeout configurable via `McpConfig::request_timeout_secs`

---

## Types

```rust
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub struct McpResourceInfo {
    pub uri: String,
    pub name: String,
    pub description: String,
    pub mime_type: Option<String>,
}

pub struct ResourceContent {
    pub uri: String,
    pub mime_type: Option<String>,
    pub text: Option<String>,
    pub blob: Option<Vec<u8>>,
}

pub struct ToolCallResult {
    pub content: String,
    pub is_error: bool,
}

pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub capabilities: ServerCapabilities,
}

pub struct ServerCapabilities {
    pub tools: bool,
    pub resources: bool,
    pub prompts: bool,  // tracked but unused in v1
}
```

### JSON-RPC types (internal)

```rust
struct JsonRpcRequest {
    jsonrpc: String,  // "2.0"
    id: u64,
    method: String,
    params: Option<Value>,
}

struct JsonRpcResponse {
    jsonrpc: String,
    id: u64,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

struct JsonRpcError {
    code: i32,
    message: String,
    data: Option<Value>,
}
```

---

## Adapters

### McpToolAdapter

Wraps an MCP tool as the agent's `Tool` trait:

```rust
pub struct McpToolAdapter {
    client: Arc<Mutex<McpClient>>,
    tool_info: McpToolInfo,
    server_name: String,  // for logging/error context
}

impl Tool for McpToolAdapter { ... }
```

### McpResourceAdapter

Exposes MCP resources to the agent. Resources are read-only data with their
own trait (not `Tool`):

```rust
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

impl Resource for McpResourceAdapter { ... }
```

The agent can inject resource contents into context when relevant (e.g., a
Postgres MCP exposing table schemas that the agent reads before writing queries).

---

## Connection Pool & Crash Recovery

### Pool

```rust
pub struct McpPool {
    connections: RwLock<HashMap<McpServerId, Arc<Mutex<McpClient>>>>,
    sandbox: Arc<BwrapSandbox>,
}

impl McpPool {
    /// Connect to all enabled MCP servers for a user. Lazy --- called on first request.
    pub async fn connect_user_servers(
        &self,
        servers: &[McpServerConfig],
    ) -> Result<(), McpError>;

    /// Get tools + resources from all connected servers.
    pub async fn discover(&self) -> Result<McpDiscovery, McpError>;

    /// Disconnect all servers, kill processes.
    pub async fn shutdown(&self) -> Result<(), McpError>;
}

pub struct McpDiscovery {
    pub tools: Vec<McpToolAdapter>,
    pub resources: Vec<McpResourceAdapter>,
}
```

### Lifecycle

- Pool is per-user, created lazily on first chat message that needs MCP
- Connections kept alive for the conversation duration
- Idle timeout (configurable, default 5 minutes) --- shutdown if no calls
- Explicit shutdown on conversation end

### Crash recovery

- **Detection:** stdout EOF or process exit
- **Between calls:** next call attempt triggers auto-restart (spawn +
  initialize), once. If restart fails, mark server unavailable for this
  conversation
- **During active call:** configurable timeout, return tool error to LLM
  ("tool X is currently unavailable"), no retry
- **Minimum cooldown:** configurable delay between restart attempts (default 5s,
  via `McpConfig::restart_cooldown_secs`). Prevents thrashing if a server
  crashes immediately on startup. Cooldown doubles after each consecutive
  failure (5s → 10s → 20s).
- **Backoff:** configurable consecutive failures (default 3) before giving up
  for this conversation, log warning
- **No cross-conversation state:** each conversation gets a fresh pool, no
  "banned" servers persisting

---

## Configuration

```rust
pub struct McpConfig {
    /// Timeout for individual MCP requests (default: 30s)
    pub request_timeout_secs: u32,

    /// Consecutive failures before marking server unavailable (default: 3)
    pub max_consecutive_failures: u32,

    /// Idle timeout before disconnecting an MCP server (default: 300s)
    pub idle_timeout_secs: u32,

    /// Minimum cooldown between restart attempts (default: 5s). Doubles after
    /// each consecutive failure to prevent thrashing.
    pub restart_cooldown_secs: u32,
}
```

`McpConfig` is an infrastructure/operational config section within `AppConfig`
(defined in `sober-core`, loaded from env vars at startup). It controls behavioral
defaults for MCP client operations. This is separate from per-user MCP server
registrations, which are runtime data.

### User-facing configuration (runtime, not AppConfig)

MCP server configs stored per-user in the `mcp_servers` database table
(schema defined in 003-sober-core). Loaded by `sober-mcp` at runtime when
processing requests for a specific user:

- `command`, `args`, `env`, `enabled` per server
- No command allowlist for v1 (self-hosted, single admin)
- **Test connection on add:** when a user adds an MCP server, `sober-mcp`
  attempts a connection (initialize + list_tools/list_resources) to verify
  it works before saving
- **Tool/resource visibility:** discovered tools and resources shown in the
  settings UI so users know what they're enabling

---

## Error Types

```rust
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
```

`ServerUnavailable` is used after consecutive failures exceed the configured
threshold. Maps to `AppError::Internal` since MCP failures shouldn't surface
as user-facing HTTP errors --- the agent adapts by using other tools.

---

## Dependencies

### Crate dependencies

- `sober-core` --- shared types, config
- `sober-sandbox` --- sandbox MCP server child processes
- `tokio` --- process management, async I/O
- `serde`, `serde_json` --- JSON-RPC serialization
- `tracing` --- structured logging
- `thiserror` --- error types

### Dependency flow

```
sober-agent  --> sober-mcp      (MCP tool/resource integration)
sober-mcp    --> sober-sandbox   (sandbox MCP server processes)
sober-mcp    --> sober-core      (shared types)
```

`sober-mcp` has no dependency on `sober-agent`, `sober-api`, or `sober-llm`.

---

## Impact on Existing Designs

| Design | Change |
|--------|--------|
| **012 sober-agent** | Split: MCP parts extracted here, agent parts remain in sober-agent plan |
| **013 sober-api** | MCP server config CRUD endpoints unchanged; test-connection endpoint added |
| **015 frontend** | MCP settings page gains tool/resource visibility after test connection |
| **009 sandbox** | No change --- already documents MCP server sandboxing |
| **018 observability** | MCP metrics unchanged |
