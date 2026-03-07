# 011 --- sober-agent

**Date:** 2026-03-06

MCP client design has been extracted to its own plan document. See
`010-sober-mcp/design.md`. This document covers `sober-agent` only.

---

## Deployment Model

`sober-agent` is a **standalone gRPC server binary**, not an in-process library.
It listens on a Unix domain socket at `/run/sober/agent.sock` and exposes the
`AgentService` gRPC service defined in `shared/proto/agent.proto`.

Other services (sober-api, sober-scheduler) connect as gRPC clients.

### Proto Service Definition

```protobuf
// shared/proto/agent.proto

syntax = "proto3";
package sober.agent.v1;

service AgentService {
  // Chat — server-streaming RPC. Client sends a message, server streams back
  // AgentEvents (text deltas, tool calls, done signal).
  rpc HandleMessage(HandleMessageRequest) returns (stream AgentEvent);

  // Scheduler jobs — server-streaming RPC for scheduled task execution.
  rpc ExecuteTask(ExecuteTaskRequest) returns (stream AgentEvent);

  // Wake — lightweight unary RPC for the scheduler to nudge the agent
  // (e.g., trigger self-evolution check, re-evaluate pending proposals).
  rpc WakeAgent(WakeRequest) returns (WakeResponse);
}

message HandleMessageRequest {
  string user_id = 1;
  string conversation_id = 2;
  string content = 3;
}

message ExecuteTaskRequest {
  string task_id = 1;
  string task_type = 2;
  bytes payload = 3;            // JSON-encoded task-specific data
  string caller_identity = 4;   // signed service identity token
  // Context fields for agent resolution
  optional string user_id = 5;
  optional string conversation_id = 6;
  optional string workspace_id = 7;
}

message WakeRequest {
  string reason = 1;
  string caller_identity = 2;
}

message WakeResponse {
  bool accepted = 1;
}

message AgentEvent {
  oneof event {
    TextDelta text_delta = 1;
    ToolCallStart tool_call_start = 2;
    ToolCallResult tool_call_result = 3;
    Done done = 4;
    Error error = 5;
  }
}

message TextDelta { string content = 1; }
message ToolCallStart { string name = 1; string input_json = 2; }
message ToolCallResult { string name = 1; string output = 2; }
message Done { string message_id = 1; uint32 prompt_tokens = 2; uint32 completion_tokens = 3; }
message Error { string message = 1; }
```

## Agent Loop

```
User message
  -> 1. Receive message (via gRPC HandleMessage)
  -> 2. Embed user message: call llm.embed(user_message) to get query vector
  -> 3. Load context: call context_loader.load(query_vector, scope, budget)
  -> 4. Build prompt (sober-mind assembles: resolved SOUL.md + context + history + access mask + tools)
  -> 5. Call LLM via sober-llm (OpenAI-compatible, streaming)
  -> 6. Parse response
  -> 7a. If tool_calls: execute tools, then:
         - If any tool is context-modifying: go to step 2 (full prompt rebuild with re-embed + re-load)
         - Otherwise: append tool results to message array, go to step 5 (re-call LLM without rebuild)
  -> 7b. If text: stream to client, done
  -> 8. Store messages in DB + embed in Qdrant
```

- **Max tool iterations:** 10 per user message (configurable). Prevents runaway loops.
- **Context budget:** Configurable token limit (default 4096) for memory retrieval.
- **Context-modifying tools:** Tools tagged with `context_modifying: true` in their
  `ToolMetadata` trigger a full prompt rebuild after execution. Examples: memory write,
  file write. Non-context-modifying tools (e.g., web_search, fetch_url) just append
  results and re-call the LLM.

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

The `Agent` struct is wrapped by a tonic gRPC server that implements `AgentService`.
The gRPC server delegates to `Agent` methods internally.

## Agent API

```rust
impl Agent {
    pub fn new(
        llm: Arc<dyn LlmEngine>,
        mind: Arc<Mind>,
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

The `Tool` trait, `ToolMetadata`, `ToolOutput`, and `ToolError` are defined in
`sober-core::types::tool` (see 003-sober-core/design.md). This is the canonical
location so that both `sober-agent` and `sober-mcp` (which does not depend on
`sober-agent`) can implement and use the trait.

`sober-agent` re-exports these types and adds `context_modifying: bool` to
`ToolMetadata` — tools tagged with `context_modifying: true` trigger a full
prompt rebuild (re-embed query, re-load context, re-assemble) after execution.
Examples: memory write, file write. Non-context-modifying tools (e.g.,
web_search, fetch_url) just append results and re-call the LLM.

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
- `tonic` --- gRPC server framework
- `prost` --- Protocol Buffers codegen
- `reqwest` --- HTTP client for web_search and fetch_url tools
- `sqlx` --- message storage
- `tokio` --- async runtime
- `serde`, `serde_json` --- serialization
- `tracing` --- structured logging
