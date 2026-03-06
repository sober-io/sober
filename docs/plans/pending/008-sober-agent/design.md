# 008 --- sober-agent + sober-mcp

**Date:** 2026-03-06

This design covers both `sober-agent` and `sober-mcp` in a single document since
they are tightly coupled: `sober-agent` depends on `sober-mcp` for MCP tool
integration.

---

## sober-agent

### Agent Loop

```
User message
  -> 1. Load context (ContextLoader from sober-memory: Qdrant search + recent DB messages)
  -> 2. Build prompt (system prompt + context + conversation history + tool definitions)
  -> 3. Call LLM via sober-llm (OpenAI-compatible, streaming)
  -> 4. Parse response
  -> 5a. If tool_calls: execute tools, append tool results, go to step 2
  -> 5b. If text: stream to client, done
  -> 6. Store messages in DB + embed in Qdrant
```

- **Max tool iterations:** 10 per user message (configurable). Prevents runaway loops.
- **Context budget:** Configurable token limit (default 4096) for memory retrieval.

### Agent Struct

```rust
pub struct Agent {
    llm: Arc<dyn LlmEngine>,
    memory: Arc<MemoryStore>,
    context_loader: Arc<ContextLoader>,
    tools: Vec<Arc<dyn Tool>>,
    db: PgPool,
    config: AgentConfig,
}
```

`AgentConfig` fields:
- `max_tool_iterations` --- hard ceiling on tool call rounds per user message
- `context_token_budget` --- token budget for memory retrieval
- `conversation_history_limit` --- max messages loaded from conversation history
- `system_prompt` --- the agent's system prompt

### Agent API

```rust
impl Agent {
    pub fn new(
        llm: Arc<dyn LlmEngine>,
        memory: Arc<MemoryStore>,
        context_loader: Arc<ContextLoader>,
        tools: Vec<Arc<dyn Tool>>,
        db: PgPool,
        config: AgentConfig,
    ) -> Self;

    pub async fn handle_message(
        &self,
        user_id: UserId,
        conversation_id: ConversationId,
        content: &str,
    ) -> Result<AgentResponseStream, AgentError>;
}
```

`AgentResponseStream` is a `Stream` of `AgentEvent`.

### AgentEvent

```rust
pub enum AgentEvent {
    TextDelta(String),
    ToolCallStart { name: String, input: serde_json::Value },
    ToolCallResult { name: String, output: String },
    Done { message_id: MessageId, usage: Usage },
}
```

### Tool Trait

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError>;
}

pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}
```

### v1 Built-in Tools

#### 1. web_search

Searches the web via a local SearXNG instance.

- **Input:** `{ query: String, max_results?: u32 }`
- **Implementation:** GET `{SEARXNG_URL}/search?q={query}&format=json`
- **Output:** Top N results formatted as text (title + URL + snippet)

#### 2. fetch_url

Fetches and extracts text content from a URL.

- **Input:** `{ url: String }`
- **Constraints:**
  - 10s request timeout
  - 1MB response size limit
  - Content-type filtering: `text/html`, `text/plain`, `application/json` only
- **Implementation:** Uses `reqwest`. HTML is converted to plain text via lightweight
  tag stripping. Output truncated to 8000 characters.

#### 3. MCP tools

Dynamically discovered from user-configured MCP servers.

- Loaded per-user from the `mcp_servers` database table
- Proxied through the `sober-mcp` client
- Wrapped via `McpToolAdapter` to conform to the `Tool` trait

### Tool Registry

```rust
pub struct ToolRegistry {
    builtin: Vec<Arc<dyn Tool>>,
}
```

- Built-in tools (`web_search`, `fetch_url`) are registered at startup.
- MCP tools are loaded per-request based on the user's enabled MCP server
  configurations.
- All tools are passed to the LLM as OpenAI-format function definitions.

---

## sober-mcp

MCP client supporting stdio transport (v1 scope).

### MCP Client

- Spawns the MCP server as a child process (command + args from `mcp_servers` table)
- Communicates via JSON-RPC 2.0 over stdin/stdout
- Lifecycle: initialize -> list tools -> execute tool calls -> shutdown

### McpClient Struct

```rust
pub struct McpClient {
    process: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}
```

### API

```rust
impl McpClient {
    pub async fn connect(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self, McpError>;

    pub async fn initialize(&mut self) -> Result<ServerInfo, McpError>;

    pub async fn list_tools(&mut self) -> Result<Vec<McpToolInfo>, McpError>;

    pub async fn call_tool(
        &mut self,
        name: &str,
        input: serde_json::Value,
    ) -> Result<String, McpError>;

    pub async fn shutdown(self) -> Result<(), McpError>;
}
```

`McpToolInfo` contains: `name`, `description`, `input_schema`.

### McpToolAdapter

Wraps an MCP tool so it implements the `Tool` trait, allowing the agent to use
MCP tools uniformly alongside built-in tools.

```rust
pub struct McpToolAdapter {
    client: Arc<Mutex<McpClient>>,
    tool_info: McpToolInfo,
}

impl Tool for McpToolAdapter { ... }
```

### MCP Connection Pool

- MCP servers are connected per-user, lazily (on first request that needs them).
- Connections are kept alive for the duration of the user's session/conversation.
- Shutdown when the conversation ends or on idle timeout.

---

## Error Types

### AgentError

```rust
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("tool execution failed: {0}")]
    ToolExecutionFailed(String),
    #[error("max tool iterations exceeded ({0})")]
    MaxIterationsExceeded(u32),
    #[error("context load failed: {0}")]
    ContextLoadFailed(String),
    #[error("LLM call failed: {0}")]
    LlmCallFailed(String),
}
```

### McpError

```rust
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("initialize failed: {0}")]
    InitializeFailed(String),
    #[error("tool call failed: {0}")]
    ToolCallFailed(String),
    #[error("protocol error: {0}")]
    ProtocolError(String),
    #[error("timeout")]
    Timeout,
}
```

Both error types map to `AppError` from `sober-core`.

---

## Dependencies

### sober-agent

- `sober-core` --- shared types, error handling
- `sober-llm` --- LLM engine abstraction
- `sober-memory` --- context loading, vector storage
- `sober-mcp` --- MCP client for external tools
- `sober-sandbox` --- process sandboxing for tool execution and artifact runs
- `reqwest` --- HTTP client for web_search and fetch_url tools
- `sqlx` --- message storage
- `tokio` --- async runtime, spawn for MCP processes
- `serde`, `serde_json` --- serialization
- `tracing` --- structured logging

### sober-mcp

- `sober-core` --- shared types
- `sober-sandbox` --- sandbox MCP server child processes
- `tokio` --- process management, async I/O
- `serde`, `serde_json` --- JSON-RPC serialization
- `tracing` --- structured logging
