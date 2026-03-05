# 007 — sober-llm: Implementation Plan

**Date:** 2026-03-06
**Design:** [design.md](./design.md)

---

## Goal

Implement the LLM abstraction layer (`sober-llm` crate) with an `LlmEngine` trait
and a single `OpenAiCompatibleEngine` client that works with any OpenAI-compatible
provider (OpenRouter, Ollama, OpenAI, etc.).

---

## Steps

### 1. Scaffold the crate

Add `sober-llm` to the Cargo workspace. Configure `Cargo.toml` with dependencies:

- `sober-core` (path dependency)
- `reqwest` (features: `rustls-tls`, `json`, `stream`)
- `tokio` (features: `rt`, `macros`)
- `serde`, `serde_json`
- `futures`
- `async-trait`
- `tracing`
- `thiserror`

### 2. Create module structure

```
backend/crates/sober-llm/src/
  lib.rs              -- Public API, re-exports
  error.rs            -- LlmError enum
  types.rs            -- All request/response types (OpenAI format)
  engine.rs           -- LlmEngine trait definition
  client.rs           -- OpenAiCompatibleEngine implementation
  streaming.rs        -- SSE parser for OpenAI streaming format
```

No provider-specific subdirectories — one client handles all providers.

### 3. Implement error.rs

- Define `LlmError` enum: `ApiError`, `RateLimited`, `NetworkError`, `StreamError`,
  `Unsupported`, `InvalidResponse`
- Implement `From<LlmError>` for `AppError` (maps to `AppError::Internal`)

### 4. Implement types.rs

All types match the OpenAI Chat Completions API JSON format:

- `CompletionRequest`: model, messages, tools, max_tokens, temperature, stop, stream
- `Message`: role (String), content, tool_calls, tool_call_id
- `ToolDefinition` + `FunctionDefinition`: function-type tool definitions
- `ToolCall` + `FunctionCall`: tool call in assistant responses
- `CompletionResponse`: id, choices, usage
- `Choice`: index, message, finish_reason
- `StreamChunk`: id, choices (with delta), usage
- `StreamChoice` + `MessageDelta` + `ToolCallDelta` + `FunctionCallDelta`
- `Usage`: prompt_tokens, completion_tokens, total_tokens
- `EngineCapabilities`: supports_tools, supports_streaming, supports_embeddings,
  max_context_tokens
- `EmbeddingRequest` + `EmbeddingResponse`: for the /embeddings endpoint

All types derive `Debug`, `Clone`, `Serialize`, `Deserialize`. Use `#[serde(skip_serializing_if)]`
for optional fields to keep requests clean.

### 5. Implement engine.rs

- Define `LlmEngine` trait with `complete()`, `stream()`, `embed()`, `capabilities()`,
  `model_id()`
- Ensure the trait is object-safe (`dyn LlmEngine` must work)

### 6. Implement client.rs

`OpenAiCompatibleEngine` struct:
- Fields: `reqwest::Client`, `base_url`, `api_key` (Option), `model`, `default_max_tokens`
- Constructor: `new(base_url, api_key, model, max_tokens) -> Self`
- `from_config(config: &LlmConfig) -> Self` convenience constructor
- `LlmEngine::complete()`:
  - Build `CompletionRequest` with `stream: false`
  - POST to `{base_url}/chat/completions`
  - Set `Authorization: Bearer {api_key}` header
  - Detect OpenRouter base URL → add `HTTP-Referer` and `X-Title` headers
  - Parse response as `CompletionResponse`
  - Handle error responses (non-2xx): parse error body, map to `LlmError::ApiError`
  - Handle 429: parse `retry-after`, return `LlmError::RateLimited`
- `LlmEngine::stream()`:
  - Same request but `stream: true`
  - Return SSE stream via `streaming.rs`
- `LlmEngine::embed()`:
  - POST to `{base_url}/embeddings`
  - If 404 or unsupported, return `LlmError::Unsupported`
- `capabilities()`: report based on known provider capabilities or probe

### 7. Implement streaming.rs

- Parse SSE format from `reqwest::Response` byte stream
- Handle `data: {json}` lines — deserialize as `StreamChunk`
- Handle `data: [DONE]` — signal end of stream
- Ignore empty lines and `event:` lines (some providers send these)
- Yield `Result<StreamChunk, LlmError>` items
- Handle malformed SSE gracefully (log warning, skip)

### 8. Unit tests

- Type serialization: roundtrip all types through serde_json
- Request building: verify `CompletionRequest` serializes to valid OpenAI JSON
- Response parsing: deserialize sample OpenAI API responses (completions, tool use, streaming)
- SSE parsing: feed mock SSE data through parser, verify correct `StreamChunk` sequence
  - Text-only response
  - Tool use response (with `tool_calls` in delta)
  - Mixed text + tool response
  - Empty delta handling
  - `[DONE]` termination
- Error mapping: verify `LlmError` variants map to correct `AppError`
- OpenRouter header detection: verify headers are added when base_url contains "openrouter"

### 9. Integration test

- Requires `LLM_BASE_URL` and `LLM_API_KEY` env vars; skip with `#[ignore]` if not set
- Send a simple completion ("What is 2+2?"), verify response has text content
- Send a streaming completion, collect all chunks, verify text is non-empty
- If tools are supported: send a tool-use request with a dummy tool, verify response
  contains tool_calls
- Verify `Usage` fields are populated

### 10. Verify

- `cargo clippy -p sober-llm -- -D warnings` — clean
- `cargo test -p sober-llm` — all unit tests pass
- `cargo doc -p sober-llm --no-deps` — all public items documented

---

## Acceptance Criteria

- [ ] `LlmEngine` trait compiles and is object-safe (usable as `dyn LlmEngine`)
- [ ] `OpenAiCompatibleEngine` works with OpenRouter (tested manually or via integration test)
- [ ] SSE streaming parser handles the OpenAI streaming format correctly
- [ ] Tool definitions serialize to the correct OpenAI `tools` format
- [ ] Tool call responses deserialize correctly (including `arguments` as JSON string)
- [ ] Rate limit errors include `retry_after` when the header is present
- [ ] `embed()` works when provider supports it, returns `Unsupported` when not
- [ ] `cargo clippy` is clean with `-D warnings`
- [ ] All public items have doc comments
