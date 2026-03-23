# Host Functions & Capabilities

Plugins communicate with the host through a fixed set of host functions
registered in the `"sober"` Extism namespace. All communication crosses the
WASM boundary as JSON — the plugin serialises a request struct, the host
deserialises it, performs the operation, and serialises the result back.

`host_log` is always available. Every other function requires the corresponding
capability to be declared in `[capabilities]` in `plugin.toml`. Calling a
function without the capability returns an error object:

```json
{ "error": "capability denied: <capability_name>" }
```

Use `sober-pdk` in your `Cargo.toml` to access all capabilities through
idiomatic Rust wrappers:

```toml
[dependencies]
sober-pdk = "0.1.0"
```

---

## `host_log` — Structured logging

**Capability:** always available (no declaration needed)

Emits a structured log entry that appears in the host's `tracing` output with
the plugin ID attached.

**Input**

```json
{
  "level":   "info",
  "message": "fetched 42 articles",
  "fields":  { "feed_url": "https://example.com/feed.xml" }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `level` | string | yes | One of `trace`, `debug`, `info`, `warn`, `error`. Unknown values fall through to `info`. |
| `message` | string | yes | Log message body. |
| `fields` | object | no | Arbitrary structured fields attached to the log entry. |

**Output**

```json
{ "ok": true }
```

**Example (Rust)**

```rust
use sober_pdk::log;

log::info("starting up");
log::warn("retrying after timeout");
log::error("unrecoverable failure");
log::debug("processing item");
```

---

## `host_kv_get` / `host_kv_set` / `host_kv_delete` / `host_kv_list` — Key-value storage

**Capability:** `key_value`

Plugin-scoped key-value storage for persistent state. Keys are namespaced to
the plugin; two plugins cannot read each other's keys. Values are any JSON
value (string, number, object, array, null).

**`host_kv_get` input / output**

```json
{ "key": "last_run_timestamp" }
// → { "value": "2026-03-01T12:00:00Z" }
```

`value` is `null` when the key does not exist.

**`host_kv_set` input / output**

```json
{ "key": "last_run_timestamp", "value": "2026-03-23T09:00:00Z" }
// → { "ok": true }
```

**`host_kv_delete` input / output**

```json
{ "key": "last_run_timestamp" }
// → { "ok": true }
```

**`host_kv_list` input / output**

```json
{ "prefix": "cache:" }
// → { "keys": ["cache:article_1", "cache:article_2"] }
```

`prefix` is optional. Omit it (or pass `null`) to list all keys.

**Example (Rust)**

```rust
use sober_pdk::kv;

// Store a counter
kv::set("counter", &serde_json::json!(42))?;

// Read it back — returns Ok(None) if the key does not exist
let val = kv::get("counter")?;

// List all keys under a prefix
let keys = kv::list(Some("cache:"))?;

// Delete a key (no error if it doesn't exist)
kv::delete("counter")?;
```

---

## `host_http_request` — Outbound HTTP

**Capability:** `network`

Makes a synchronous outbound HTTP request. When the manifest declares
`network = { allowed_hosts = [...] }`, the target hostname must match an
entry in that list.

Supported methods: `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS`.

**Input**

```json
{
  "method":  "GET",
  "url":     "https://api.example.com/data",
  "headers": { "Accept": "application/json" },
  "body":    null
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `method` | string | yes | HTTP method (case-insensitive). |
| `url` | string | yes | Full URL including scheme. |
| `headers` | object | no | Key-value map of request headers. |
| `body` | string or null | no | Request body as a UTF-8 string. |

**Output (success)**

```json
{
  "status":  200,
  "headers": { "content-type": "application/json" },
  "body":    "{\"result\": 42}"
}
```

**Output (error)**

```json
{ "error": "HTTP request failed: connection refused" }
```

HTTP 4xx and 5xx responses are returned as success from the host function
perspective — the plugin sees the status code and decides how to handle it.

**Example (Rust)**

```rust
use sober_pdk::http;

// Simple GET
let resp = http::get("https://api.example.com/data", &[])?;
assert_eq!(resp.status, 200);
let body: serde_json::Value = serde_json::from_str(&resp.body)?;

// POST with headers and body
let resp = http::post(
    "https://api.example.com/submit",
    &[("Content-Type", "application/json")],
    r#"{"key": "value"}"#,
)?;

// Arbitrary method
let resp = http::request("PUT", "https://api.example.com/item/1", &[], Some(r#""data""#))?;
```

---

## `host_emit_metric` — Emit a Prometheus metric

**Capability:** `metrics`

Emits a counter, gauge, or histogram observation. The metric name must be
declared in `[[metrics]]` in the manifest.

**Input**

```json
{
  "name":   "articles_indexed_total",
  "kind":   "counter",
  "value":  1.0,
  "labels": { "feed": "tech" }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Must match a `[[metrics]]` declaration. |
| `kind` | string | yes | One of `counter`, `gauge`, `histogram`. |
| `value` | number | yes | Observation value. |
| `labels` | object | no | Prometheus label key-value pairs. |

**Output**

```json
{ "ok": true }
```

**Example (Rust)**

```rust
use sober_pdk::metrics;

// Increment a counter
metrics::emit("requests_total", "counter", 1.0, &[("method", "GET")])?;

// Set a gauge
metrics::emit("queue_depth", "gauge", 42.0, &[])?;

// Record a histogram sample
metrics::emit("response_time_ms", "histogram", 23.5, &[("endpoint", "/api")])?;
```

---

## `host_fs_read` / `host_fs_write` — Sandboxed filesystem

**Capability:** `filesystem`

Read and write files inside the sandbox. Paths must start with one of the
prefixes declared in `filesystem.allowed_paths`. Write access additionally
requires `writable = true` in the capability declaration.

**`host_fs_read` input / output**

```json
{ "path": "/tmp/my-plugin/data.txt" }
// → { "content": "line one\nline two\n" }
// → { "error": "filesystem: path not allowed" }
```

**`host_fs_write` input / output**

```json
{ "path": "/tmp/my-plugin/output.txt", "content": "hello\n" }
// → { "ok": true }
```

**Example (Rust)**

```rust
use sober_pdk::fs;

fs::write("/workspace/data/output.txt", "hello")?;
let content = fs::read("/workspace/data/output.txt")?;
```

---

## `host_read_secret` — Read a secret

**Capability:** `secret_read`

Looks up a named secret from the host vault. Secrets are stored encrypted and
are never logged. The plugin receives the cleartext value — the host handles
decryption.

**Input / output**

```json
{ "name": "OPENAI_API_KEY" }
// → { "value": "sk-..." }
// → { "error": "secret not found: OPENAI_API_KEY" }
```

**Example (Rust)**

```rust
use sober_pdk::secret;

let api_key = secret::read("API_KEY")?;
```

---

## `host_call_tool` — Call another tool

**Capability:** `tool_call`

Invokes any registered tool by name. When the manifest declares
`tool_call = { allowed_tools = [...] }`, only the listed tools may be called.

**Input / output**

```json
{ "tool": "web_search", "input": { "query": "latest Rust releases" } }
// → { "output": { "results": [ ... ] } }
// → { "error": "tool not found: unknown_tool" }
```

**Example (Rust)**

```rust
use sober_pdk::tool;

let result = tool::call("web_search", serde_json::json!({ "query": "Rust WASM" }))?;
let items = result["results"].as_array();
```

---

## `host_memory_query` / `host_memory_write` — Vector memory

**Capability:** `memory_read` / `memory_write`

Read and write access to the Sõber vector memory system. Queries perform
semantic similarity search; writes embed and index the content for future
retrieval.

**`host_memory_query` input / output**

```json
{
  "query": "Rust async runtime comparison",
  "scope": "user",
  "limit": 5
}
// → {
//     "results": [
//       { "content": "Tokio is the most widely used async runtime...", "score": 0.92 },
//       { "content": "async-std provides an alternative...", "score": 0.87 }
//     ]
//   }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `query` | string | yes | Natural language query for similarity search. |
| `scope` | string | no | Memory scope to search (`"user"`, `"group"`, `"session"`, `"system"`). Searches all accessible scopes when omitted. |
| `limit` | integer | no | Maximum number of results. Defaults to a system-defined limit. |

**`host_memory_write` input / output**

```json
{
  "content":  "Tokio 1.36 was released with improved task scheduling.",
  "scope":    "user",
  "metadata": { "source": "rust-blog", "date": "2026-03-01" }
}
// → { "ok": true }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `content` | string | yes | Text to embed and store. |
| `scope` | string | no | Target memory scope. Defaults to `"user"`. |
| `metadata` | object | no | Arbitrary key-value metadata attached to the chunk. |

**Example (Rust)**

```rust
use sober_pdk::memory;
use std::collections::HashMap;

// Semantic search — returns Vec<MemoryHit> with .content, .score, .chunk_type
let hits = memory::query("recent meetings", None, Some(5))?;
for hit in &hits {
    sober_pdk::log::info(&format!("score {}: {}", hit.score, hit.content));
}

// Write a new chunk to the user scope
let mut meta = HashMap::new();
meta.insert("source".to_string(), serde_json::json!("my-plugin"));
memory::write("User prefers dark mode", Some("user"), meta)?;
```

---

## `host_conversation_read` — Read conversation history

**Capability:** `conversation_read`

Returns the most recent messages from a conversation.

**Input / output**

```json
{ "conversation_id": "01924abc-...", "limit": 20 }
// → {
//     "messages": [
//       { "role": "user",      "content": "What is the weather today?", "created_at": "..." },
//       { "role": "assistant", "content": "I'll check that for you.",   "created_at": "..." }
//     ]
//   }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `conversation_id` | string | yes | UUID of the conversation. |
| `limit` | integer | no | Maximum number of messages to return (most recent first). |

**Example (Rust)**

```rust
use sober_pdk::conversation;

// Returns Vec<ConversationMessage> with .role, .content, .created_at
let messages = conversation::read("01924abc-...", Some(10))?;
for msg in &messages {
    sober_pdk::log::debug(&format!("{}: {}", msg.role, msg.content));
}
```

---

## `host_schedule` — Create a scheduled job

**Capability:** `schedule`

Registers a one-time or recurring job with the scheduler. The `schedule` field
accepts a cron expression or a duration string.

**Input / output**

```json
{
  "schedule": "0 9 * * 1-5",
  "payload":  { "action": "send_summary", "channel": "general" }
}
// → { "job_id": "01924abc-..." }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `schedule` | string | yes | Cron expression (`"*/5 * * * *"`) or interval (`"30s"`, `"1h"`). |
| `payload` | any JSON | yes | Data delivered to the job handler when the job fires. |

**Example (Rust)**

```rust
use sober_pdk::schedule;

// Schedule a recurring job — returns the job ID string
let job_id = schedule::add("*/5 * * * *", &serde_json::json!({"task": "cleanup"}))?;
sober_pdk::log::info(&format!("scheduled job {job_id}"));
```

---

## `host_llm_complete` — LLM completion

**Capability:** `llm_inference`

Sends a prompt to the agent's configured LLM provider and returns the
generated text. By default the agent's system prompt is included for consistent
behavior. Minimum `max_tokens` is enforced at 4096 to give thinking models
enough headroom.

**Input**

```json
{
  "prompt":     "Summarise the following article in three sentences: ...",
  "model":      "claude-3-7-sonnet-20250219",
  "max_tokens": 512,
  "raw":        false
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `prompt` | string | yes | User-turn prompt text. |
| `model` | string | no | Model identifier. Defaults to the agent's configured model. |
| `max_tokens` | integer | no | Maximum tokens in the response. Clamped to at least 4096. |
| `raw` | bool | no | When `true`, omits the agent's system prompt. Default `false`. |

**Output**

```json
{ "text": "The article discusses..." }
```

`text` is an empty string if the model returned no content (e.g. finish reason
was `stop` with no output). The host logs a warning in this case.

**Example (Rust)**

```rust
use sober_pdk::llm;

// Complete with agent's system prompt included
let summary = llm::complete("Summarise this text: ...", None, Some(512))?;

// Raw completion — full control over context, no system prompt injected
let raw = llm::complete_raw("You are a JSON formatter. Return only JSON.", None, None)?;

// Target a specific model
let response = llm::complete("Explain WASM", Some("claude-3-7-sonnet-20250219"), None)?;
```
