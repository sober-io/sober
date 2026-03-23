# Manifest Reference

Every WASM plugin ships a `plugin.toml` file that declares its identity,
the tools it exposes, the host capabilities it requires, and any metrics it
intends to emit.

The manifest is parsed by `PluginManifest::from_toml` at install time. Parsing
failures or validation errors reject the plugin before the audit pipeline runs.

## Validation rules

- `plugin.name` must not be empty.
- At least one `[[tools]]` entry must be declared.
- If `[capabilities] metrics = true` is set, at least one `[[metrics]]` entry
  must also be present.

---

## `[plugin]` — Core metadata

```toml
[plugin]
name        = "my-plugin"        # required — unique identifier within the registry
version     = "1.2.0"            # required — semantic version string
description = "What it does"     # optional
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Human-readable plugin name. Used as identifier in tool routing. |
| `version` | string | yes | Semantic version. Displayed in the registry and audit log. |
| `description` | string | no | Short description shown in the UI and tool list. |

---

## `[[tools]]` — Tool declarations

Each entry declares one tool the plugin exports. The `name` value must match
the Rust function name (using underscores, not hyphens).

```toml
[[tools]]
name        = "fetch_page"
description = "Fetches the text content of a web page"

[[tools]]
name        = "summarise"
description = "Summarises a piece of text using an LLM"
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Must match the `#[plugin_fn]` export name. Hyphens are normalised to underscores at call time. |
| `description` | string | yes | Shown to the agent when selecting tools. Write it as a clear, imperative sentence. |

---

## `[capabilities]` — Capability declarations

All capability fields default to `false` when absent. A capability can be set
to `true` (enable with no restrictions) or to a config object (enable with
restrictions).

```toml
[capabilities]
key_value         = true
memory_read       = true
memory_write      = { scopes = ["user"] }
network           = { allowed_hosts = ["api.example.com", "search.example.org"] }
filesystem        = { allowed_paths = ["/tmp/my-plugin"], writable = true }
llm_call          = true
tool_call         = { allowed_tools = ["web_search", "calculator"] }
conversation_read = true
metrics           = true   # requires [[metrics]] entries
secret_read       = true
schedule          = true
```

### Capability keys

| Key | Value type | Description |
|-----|-----------|-------------|
| `key_value` | `bool` | Plugin-scoped key-value storage (get, set, delete, list). |
| `memory_read` | `bool` or `{ scopes = [...] }` | Read from vector memory. `scopes` restricts which memory scopes are accessible. |
| `memory_write` | `bool` or `{ scopes = [...] }` | Write to vector memory. |
| `network` | `bool` or `{ allowed_hosts = [...] }` | Outbound HTTP requests. An empty `allowed_hosts` means all domains are permitted. |
| `filesystem` | `bool` or `{ allowed_paths = [...], writable = false }` | Filesystem access. `allowed_paths` restricts path prefixes. `writable` enables writes (default `false`). |
| `llm_call` | `bool` | Send prompts to an LLM provider via the agent's configured engine. Alias: `llm_inference`. |
| `tool_call` | `bool` or `{ allowed_tools = [...] }` | Call other registered tools. `allowed_tools` restricts which tools may be called. |
| `conversation_read` | `bool` | Read conversation history for a given conversation ID. |
| `metrics` | `bool` | Emit Prometheus-style metrics. Requires `[[metrics]]` declarations. |
| `secret_read` | `bool` | Look up secrets from the vault by name. |
| `schedule` | `bool` | Create scheduled jobs (cron or interval). |

### Capability config details

**`memory_read` / `memory_write`**

```toml
# All scopes
memory_read = true

# Restricted to user and session scopes
memory_write = { scopes = ["user", "session"] }
```

Valid scope names: `"user"`, `"group"`, `"session"`, `"system"`.

**`network`**

```toml
# All domains permitted
network = true

# Domain-restricted
network = { allowed_hosts = ["api.openweathermap.org"] }
```

The host enforces restriction by extracting the hostname from the request URL
and comparing it against `allowed_hosts`. The port is stripped before comparison.

**`filesystem`**

```toml
# Read-only access under /tmp/my-plugin
filesystem = { allowed_paths = ["/tmp/my-plugin"] }

# Read-write access
filesystem = { allowed_paths = ["/tmp/my-plugin"], writable = true }
```

**`tool_call`**

```toml
# Any tool
tool_call = true

# Restricted tool set
tool_call = { allowed_tools = ["web_search", "calculator"] }
```

---

## `[[metrics]]` — Metric declarations

Required when `[capabilities] metrics = true`. Each entry pre-declares a metric
the plugin will emit via `host_emit_metric`.

```toml
[[metrics]]
name        = "pages_fetched_total"
kind        = "counter"
description = "Total number of pages fetched"

[[metrics]]
name        = "fetch_duration_seconds"
kind        = "histogram"
description = "Time taken to fetch a page"
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Prometheus-style metric name. Use `snake_case` and standard suffixes (`_total`, `_seconds`, `_bytes`). |
| `kind` | string | yes | One of `counter`, `gauge`, or `histogram`. |
| `description` | string | yes | Human-readable description shown in the metrics registry. |

---

## Complete example

```toml
[plugin]
name        = "news-fetcher"
version     = "0.3.1"
description = "Fetches and indexes news articles"

[capabilities]
network       = { allowed_hosts = ["feeds.example.com", "api.example.com"] }
memory_write  = { scopes = ["user"] }
key_value     = true
metrics       = true

[[tools]]
name        = "fetch_feed"
description = "Fetches articles from an RSS feed URL and indexes them into memory"

[[tools]]
name        = "list_articles"
description = "Lists recently indexed article titles and URLs"

[[metrics]]
name        = "articles_indexed_total"
kind        = "counter"
description = "Total number of articles written to memory"

[[metrics]]
name        = "feed_fetch_errors_total"
kind        = "counter"
description = "Total number of feed fetch failures"
```
