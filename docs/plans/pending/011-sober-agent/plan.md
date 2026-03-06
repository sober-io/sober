# 011 --- sober-agent: Implementation Plan

**Date:** 2026-03-06

**Depends on:** sober-mcp (010), sober-mind (009), sober-llm (007),
sober-memory (006), sober-sandbox (008)

MCP implementation has been extracted to its own plan. See
`010-sober-mcp/` for the MCP client implementation plan.

---

### 1. Define agent.proto

Create `shared/proto/agent.proto` with the `AgentService` gRPC service definition:
- `HandleMessage` — server-streaming RPC for chat (request: user_id, conversation_id, content)
- `ExecuteTask` — server-streaming RPC for scheduler jobs (request: task_id, task_type, payload)
- `AgentEvent` — response message with oneof: TextDelta, ToolCallStart, ToolCallResult, Done, Error

### 2. Add dependencies to sober-agent Cargo.toml

- `sober-core`, `sober-llm`, `sober-memory`, `sober-mcp`, `sober-mind` (path dependencies)
- `tonic` — gRPC server framework
- `prost` — Protocol Buffers codegen
- `tonic-build` (build dependency) — proto compilation
- `reqwest` with `json` feature
- `sqlx` with `runtime-tokio`, `postgres` features
- `tokio`
- `serde`, `serde_json`
- `tracing`
- `thiserror`
- `async-trait`
- `futures` (for Stream types)

### 3. Create module structure

```
sober-agent/src/
  main.rs       # Binary entry point — starts tonic gRPC server on /run/sober/agent.sock
  lib.rs
  error.rs
  agent.rs
  grpc.rs       # tonic service impl wrapping Agent
  stream.rs
  tools/
    mod.rs
    web_search.rs
    fetch_url.rs
    registry.rs
  build.rs      # tonic-build proto compilation
```

### 4. Implement error.rs

- `AgentError` enum: `ToolExecutionFailed`, `MaxIterationsExceeded`,
  `ContextLoadFailed`, `LlmCallFailed`
- `From<AgentError>` for `AppError`

### 5. Implement Tool trait in tools/mod.rs

- `Tool` trait definition (`metadata()` returning `ToolMetadata`, `execute()`)
- `ToolMetadata` struct (name, description, input_schema, context_modifying)
- `ToolOutput` struct (content, is_error)
- `ToolError` type
- Re-export from crate root

### 6. Implement tools/web_search.rs

- `WebSearchTool` struct holding SearXNG base URL and reqwest client
- Input parsing: `query` (required), `max_results` (optional, default 5)
- GET request to SearXNG JSON API
- Response parsing: extract title, URL, snippet from each result
- Format as readable text output

### 7. Implement tools/fetch_url.rs

- `FetchUrlTool` struct holding reqwest client (configured with 10s timeout, 1MB limit)
- Input parsing: `url` (required)
- Content-type validation: allow `text/html`, `text/plain`, `application/json` only
- HTML to plain text: lightweight tag stripping
- Output truncation to 8000 characters

### 8. Implement tools/registry.rs

- `ToolRegistry` struct holding `Vec<Arc<dyn Tool>>` for built-in tools
- `ToolRegistry::new()` --- register built-in tools (web_search, fetch_url)
- `ToolRegistry::with_mcp_tools(mcp_tools)` --- merge MCP tools for a specific request
- `ToolRegistry::tool_definitions()` --- return OpenAI-format function definitions for all tools
- `ToolRegistry::get_tool(name)` --- look up a tool by name

### 9. Implement agent.rs

- `Agent` struct and `AgentConfig`
- `Agent::new(...)` constructor (includes `mind: Arc<Mind>` parameter)
- `Agent::handle_message(user_id, conversation_id, content)`:
  1. Embed user message: call `llm.embed(content)` to get query vector
  2. Load context via `context_loader.load(query_vector, scope, budget)`
  3. Build prompt via `mind.assemble(...)` (resolved SOUL.md + context + history + access mask + tools)
  4. Enter loop (up to `max_tool_iterations`):
     a. Call LLM via `sober-llm` (streaming)
     b. If response contains tool_calls: execute each tool, then:
        - If any executed tool has `context_modifying: true`: go to step 1 (full prompt rebuild)
        - Otherwise: append tool results to message array, continue loop (re-call LLM without rebuild)
     c. If response is text: stream to client, break
  5. Store assistant message in DB
  6. Embed message in Qdrant for future retrieval
- Tool execution: look up tool by name in registry, call `execute()`, handle errors

### 10. Implement grpc.rs

- Implement `AgentService` tonic trait wrapping the `Agent` struct
- `handle_message` — delegates to `Agent::handle_message`, maps `AgentEvent` stream to gRPC response stream
- `execute_task` — delegates to agent for scheduler-driven tasks

### 11. Implement main.rs

- Parse config (socket path, DB URL, etc.)
- Init tracing
- Connect to PostgreSQL, Qdrant, create LLM engine, Mind, MemoryStore, ContextLoader
- Create `Agent` instance
- Wrap in `AgentServiceServer`
- Bind tonic server to Unix domain socket at `/run/sober/agent.sock`
- Graceful shutdown on SIGTERM/SIGINT

### 12. Implement stream.rs

- `AgentEvent` enum: `TextDelta`, `ToolCallStart`, `ToolCallResult`, `Done`
- `AgentResponseStream` type alias: `Pin<Box<dyn Stream<Item = Result<AgentEvent, AgentError>> + Send>>`

### 13. Unit tests

- Tool input validation (web_search rejects empty query, fetch_url rejects invalid URLs)
- web_search response parsing from sample SearXNG JSON
- fetch_url content extraction: HTML tag stripping, size truncation, content-type rejection
- Tool registry: built-in registration, MCP tool merging, lookup by name
- AgentConfig defaults

### 14. Integration tests

- Full agent loop with mock LLM returning text-only response
- Full agent loop with mock LLM returning tool_calls followed by text response
- Agent loop hitting max iterations (mock LLM always returns tool_calls)
- Context-modifying tool triggers full prompt rebuild (verify embed + context reload is called again)
- Non-context-modifying tool appends results without rebuild
- gRPC server accepts HandleMessage on UDS and streams AgentEvents
- Optionally: test against real LLM provider if `SOBER_TEST_LLM=1` env var is set

---

## Acceptance Criteria

- Agent binary starts and listens on `/run/sober/agent.sock` as a gRPC server.
- `HandleMessage` RPC streams `AgentEvent` messages to the client.
- Agent loop handles text-only responses correctly (stream text, store in DB).
- Agent loop handles tool calls: call tool, feed result back to LLM, get final response.
- Context-modifying tools trigger a full prompt rebuild (re-embed, re-load context).
- Non-context-modifying tools append results and re-call LLM without rebuilding.
- Agent loop stops at max iterations and returns `MaxIterationsExceeded` error.
- `web_search` tool correctly parses SearXNG JSON responses.
- `fetch_url` tool respects size limits (1MB download, 8000 char output) and
  content-type filtering (rejects non-text responses).
- MCP tools and resources are loaded via `McpPool::discover()` and merged into the tool registry.
- `cargo clippy -- -D warnings` passes.
- `cargo test --workspace` passes with all new tests green.
