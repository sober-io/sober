# 011 --- sober-agent: Implementation Plan

**Date:** 2026-03-06

**Depends on:** sober-mcp (010), sober-mind (009), sober-llm (007),
sober-memory (006), sober-sandbox (008)

MCP implementation has been extracted to its own plan. See
`010-sober-mcp/` for the MCP client implementation plan.

---

### 1. Add dependencies to sober-agent Cargo.toml

- `sober-core`, `sober-llm`, `sober-memory`, `sober-mcp` (path dependencies)
- `reqwest` with `json` feature
- `sqlx` with `runtime-tokio`, `postgres` features
- `tokio`
- `serde`, `serde_json`
- `tracing`
- `thiserror`
- `async-trait`
- `futures` (for Stream types)

### 2. Create module structure

```
sober-agent/src/
  lib.rs
  error.rs
  agent.rs
  stream.rs
  tools/
    mod.rs
    web_search.rs
    fetch_url.rs
    registry.rs
```

### 3. Implement error.rs

- `AgentError` enum: `ToolExecutionFailed`, `MaxIterationsExceeded`,
  `ContextLoadFailed`, `LlmCallFailed`
- `From<AgentError>` for `AppError`

### 4. Implement Tool trait in tools/mod.rs

- `Tool` trait definition (name, description, input_schema, execute)
- `ToolOutput` struct (content, is_error)
- `ToolError` type
- Re-export from crate root

### 5. Implement tools/web_search.rs

- `WebSearchTool` struct holding SearXNG base URL and reqwest client
- Input parsing: `query` (required), `max_results` (optional, default 5)
- GET request to SearXNG JSON API
- Response parsing: extract title, URL, snippet from each result
- Format as readable text output

### 6. Implement tools/fetch_url.rs

- `FetchUrlTool` struct holding reqwest client (configured with 10s timeout, 1MB limit)
- Input parsing: `url` (required)
- Content-type validation: allow `text/html`, `text/plain`, `application/json` only
- HTML to plain text: lightweight tag stripping
- Output truncation to 8000 characters

### 7. Implement tools/registry.rs

- `ToolRegistry` struct holding `Vec<Arc<dyn Tool>>` for built-in tools
- `ToolRegistry::new()` --- register built-in tools (web_search, fetch_url)
- `ToolRegistry::with_mcp_tools(mcp_tools)` --- merge MCP tools for a specific request
- `ToolRegistry::tool_definitions()` --- return OpenAI-format function definitions for all tools
- `ToolRegistry::get_tool(name)` --- look up a tool by name

### 8. Implement agent.rs

- `Agent` struct and `AgentConfig`
- `Agent::new(...)` constructor
- `Agent::handle_message(user_id, conversation_id, content)`:
  1. Load context via `ContextLoader` (Qdrant search + recent messages)
  2. Build messages array (system prompt, context, history, user message)
  3. Enter loop (up to `max_tool_iterations`):
     a. Call LLM via `sober-llm` (streaming)
     b. If response contains tool_calls: execute each tool, append results, continue loop
     c. If response is text: stream to client, break
  4. Store assistant message in DB
  5. Embed message in Qdrant for future retrieval
- Tool execution: look up tool by name in registry, call `execute()`, handle errors

### 9. Implement stream.rs

- `AgentEvent` enum: `TextDelta`, `ToolCallStart`, `ToolCallResult`, `Done`
- `AgentResponseStream` type alias: `Pin<Box<dyn Stream<Item = Result<AgentEvent, AgentError>> + Send>>`

### 10. Unit tests

- Tool input validation (web_search rejects empty query, fetch_url rejects invalid URLs)
- web_search response parsing from sample SearXNG JSON
- fetch_url content extraction: HTML tag stripping, size truncation, content-type rejection
- Tool registry: built-in registration, MCP tool merging, lookup by name
- AgentConfig defaults

### 11. Integration tests

- Full agent loop with mock LLM returning text-only response
- Full agent loop with mock LLM returning tool_calls followed by text response
- Agent loop hitting max iterations (mock LLM always returns tool_calls)
- Optionally: test against real LLM provider if `SOBER_TEST_LLM=1` env var is set

---

## Acceptance Criteria

- Agent loop handles text-only responses correctly (stream text, store in DB).
- Agent loop handles tool calls: call tool, feed result back to LLM, get final response.
- Agent loop stops at max iterations and returns `MaxIterationsExceeded` error.
- `web_search` tool correctly parses SearXNG JSON responses.
- `fetch_url` tool respects size limits (1MB download, 8000 char output) and
  content-type filtering (rejects non-text responses).
- MCP tools and resources are loaded via `McpPool::discover()` and merged into the tool registry.
- `cargo clippy -- -D warnings` passes.
- `cargo test --workspace` passes with all new tests green.
