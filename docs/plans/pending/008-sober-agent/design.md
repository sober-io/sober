# 008 --- sober-agent

**Date:** 2026-03-06

MCP client design has been extracted to its own plan document. See
`008-sober-mcp/design.md`. This document covers `sober-agent` only.

---

## Agent Loop

```
User message
  -> 1. Load context (ContextLoader from sober-memory: Qdrant search + recent DB messages)
  -> 2. Build prompt (sober-mind assembles: resolved SOUL.md + context + history + access mask + tools)
  -> 3. Call LLM via sober-llm (OpenAI-compatible, streaming)
  -> 4. Parse response
  -> 5a. If tool_calls: execute tools, append tool results, go to step 2
  -> 5b. If text: stream to client, done
  -> 6. Store messages in DB + embed in Qdrant
```

- **Max tool iterations:** 10 per user message (configurable). Prevents runaway loops.
- **Context budget:** Configurable token limit (default 4096) for memory retrieval.

## Agent Struct

```rust
pub struct Agent {
    llm: Arc<dyn LlmEngine>,
    mind: Arc<Mind>,
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
- `system_prompt` --- fallback system prompt (sober-mind overrides when available)

## Agent API

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

## AgentEvent

```rust
pub enum AgentEvent {
    TextDelta(String),
    ToolCallStart { name: String, input: serde_json::Value },
    ToolCallResult { name: String, output: String },
    Done { message_id: MessageId, usage: Usage },
}
```

## Tool Trait

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

## v1 Built-in Tools

### 1. web_search

Searches the web via a local SearXNG instance.

- **Input:** `{ query: String, max_results?: u32 }`
- **Implementation:** GET `{SEARXNG_URL}/search?q={query}&format=json`
- **Output:** Top N results formatted as text (title + URL + snippet)

### 2. fetch_url

Fetches and extracts text content from a URL.

- **Input:** `{ url: String }`
- **Constraints:**
  - 10s request timeout
  - 1MB response size limit
  - Content-type filtering: `text/html`, `text/plain`, `application/json` only
- **Implementation:** Uses `reqwest`. HTML is converted to plain text via lightweight
  tag stripping. Output truncated to 8000 characters.

### 3. MCP tools + resources

Dynamically discovered from user-configured MCP servers via `sober-mcp`.

- Loaded per-user from the `mcp_servers` database table
- Tools proxied through `McpPool`, wrapped via `McpToolAdapter`
- Resources proxied through `McpPool`, wrapped via `McpResourceAdapter`
- Agent can inject resource contents into context when relevant

## Tool Registry

```rust
pub struct ToolRegistry {
    builtin: Vec<Arc<dyn Tool>>,
}
```

- Built-in tools (`web_search`, `fetch_url`) are registered at startup.
- MCP tools are loaded per-request based on the user's enabled MCP server
  configurations via `McpPool::discover()`.
- All tools are passed to the LLM as OpenAI-format function definitions.

---

## Error Types

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

Maps to `AppError` from `sober-core`.

---

## Dependencies

- `sober-core` --- shared types, error handling
- `sober-mind` --- SOUL.md resolution, prompt assembly, access masks
- `sober-llm` --- LLM engine abstraction
- `sober-memory` --- context loading, vector storage
- `sober-mcp` --- MCP client for external tools and resources
- `sober-sandbox` --- process sandboxing for tool execution and artifact runs
- `reqwest` --- HTTP client for web_search and fetch_url tools
- `sqlx` --- message storage
- `tokio` --- async runtime
- `serde`, `serde_json` --- serialization
- `tracing` --- structured logging
