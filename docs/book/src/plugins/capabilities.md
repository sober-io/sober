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
use extism_pdk::*;

fn log_info(msg: &str) {
    let req = serde_json::json!({
        "level": "info",
        "message": msg,
        "fields": {}
    });
    host::call("sober", "host_log", &serde_json::to_vec(&req).unwrap()).ok();
}
```

---

## `host_kv_get` — Read a KV entry

**Capability:** `key_value`

Reads a value from plugin-scoped key-value storage. Keys are namespaced to the
plugin; two plugins cannot read each other's keys.

**Input**

```json
{ "key": "last_run_timestamp" }
```

**Output**

```json
{ "value": "2026-03-01T12:00:00Z" }
```

`value` is `null` when the key does not exist.

---

## `host_kv_set` — Write a KV entry

**Capability:** `key_value`

**Input**

```json
{ "key": "last_run_timestamp", "value": "2026-03-23T09:00:00Z" }
```

`value` is any JSON value (string, number, object, array, null).

**Output**

```json
{ "ok": true }
```

---

## `host_kv_delete` — Delete a KV entry

**Capability:** `key_value`

**Input**

```json
{ "key": "last_run_timestamp" }
```

**Output**

```json
{ "ok": true }
```

---

## `host_kv_list` — List KV keys

**Capability:** `key_value`

**Input**

```json
{ "prefix": "cache:" }
```

`prefix` is optional. Omit it (or pass `null`) to list all keys.

**Output**

```json
{ "keys": ["cache:article_1", "cache:article_2"] }
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

---

## `host_fs_read` — Read a file

**Capability:** `filesystem`

Reads the UTF-8 content of a file. The path must start with one of the prefixes
declared in `filesystem.allowed_paths`.

**Input**

```json
{ "path": "/tmp/my-plugin/data.txt" }
```

**Output (success)**

```json
{ "content": "line one\nline two\n" }
```

**Output (error)**

```json
{ "error": "filesystem: path not allowed" }
```

---

## `host_fs_write` — Write a file

**Capability:** `filesystem` with `writable = true`

Writes UTF-8 content to a file, creating it if it does not exist.

**Input**

```json
{ "path": "/tmp/my-plugin/output.txt", "content": "hello\n" }
```

**Output**

```json
{ "ok": true }
```

---

## `host_read_secret` — Read a secret

**Capability:** `secret_read`

Looks up a named secret from the host vault. Secrets are stored encrypted and
are never logged.

**Input**

```json
{ "name": "OPENAI_API_KEY" }
```

**Output (success)**

```json
{ "value": "sk-..." }
```

**Output (error)**

```json
{ "error": "secret not found: OPENAI_API_KEY" }
```

---

## `host_call_tool` — Call another tool

**Capability:** `tool_call`

Invokes any registered tool by name. When the manifest declares
`tool_call = { allowed_tools = [...] }`, only the listed tools may be called.

**Input**

```json
{ "tool": "web_search", "input": { "query": "latest Rust releases" } }
```

**Output (success)**

```json
{ "output": { "results": [ ... ] } }
```

**Output (error)**

```json
{ "error": "tool not found: unknown_tool" }
```

---

## `host_memory_query` — Search vector memory

**Capability:** `memory_read`

Performs a semantic similarity search over the vector memory store.

**Input**

```json
{
  "query": "Rust async runtime comparison",
  "scope": "user",
  "limit": 5
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `query` | string | yes | Natural language query for similarity search. |
| `scope` | string | no | Memory scope to search (`"user"`, `"group"`, `"session"`, `"system"`). Searches all accessible scopes when omitted. |
| `limit` | integer | no | Maximum number of results. Defaults to a system-defined limit. |

**Output**

```json
{
  "results": [
    { "content": "Tokio is the most widely used async runtime...", "score": 0.92 },
    { "content": "async-std provides an alternative async runtime...", "score": 0.87 }
  ]
}
```

---

## `host_memory_write` — Write to vector memory

**Capability:** `memory_write`

Stores a text chunk in vector memory. The chunk is embedded and indexed for
future similarity queries.

**Input**

```json
{
  "content":  "Tokio 1.36 was released with improved task scheduling.",
  "scope":    "user",
  "metadata": { "source": "rust-blog", "date": "2026-03-01" }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `content` | string | yes | Text to embed and store. |
| `scope` | string | no | Target memory scope. Defaults to `"user"`. |
| `metadata` | object | no | Arbitrary key-value metadata attached to the chunk. |

**Output**

```json
{ "ok": true }
```

---

## `host_conversation_read` — Read conversation history

**Capability:** `conversation_read`

Returns the most recent messages from a conversation.

**Input**

```json
{ "conversation_id": "01924abc-...", "limit": 20 }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `conversation_id` | string | yes | UUID of the conversation. |
| `limit` | integer | no | Maximum number of messages to return (most recent first). |

**Output**

```json
{
  "messages": [
    { "role": "user",      "content": "What is the weather today?" },
    { "role": "assistant", "content": "I'll check that for you." }
  ]
}
```

---

## `host_schedule` — Create a scheduled job

**Capability:** `schedule`

Registers a one-time or recurring job with the scheduler. The `schedule` field
accepts a cron expression or a duration string.

**Input**

```json
{
  "schedule": "0 9 * * 1-5",
  "payload":  { "action": "send_summary", "channel": "general" }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `schedule` | string | yes | Cron expression (`"*/5 * * * *"`) or interval (`"30s"`, `"1h"`). |
| `payload` | any JSON | yes | Data delivered to the job handler when the job fires. |

**Output**

```json
{ "job_id": "01924abc-..." }
```

---

## `host_llm_complete` — LLM completion

**Capability:** `llm_call`

Sends a prompt to the agent's configured LLM provider and returns the
generated text. Minimum `max_tokens` is enforced at 4096 to give thinking
models enough headroom.

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
