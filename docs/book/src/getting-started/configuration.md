# Configuration

Sõber uses a **layered configuration model**. Values are resolved from multiple sources, with
higher-priority layers overriding lower ones.

---

## Resolution Order

| Priority | Source | Notes |
|----------|--------|-------|
| 1 (highest) | Environment variables (`SOBER_*`) | Always win; good for secrets in CI/CD |
| 2 | `.env` file | Development convenience; loaded via dotenvy |
| 3 | `config.toml` | Primary config file for production |
| 4 (lowest) | Compiled defaults | Safe fallbacks; not suitable for production secrets |

### config.toml Discovery

Sõber searches for `config.toml` in this order:

1. Path given in the `SOBER_CONFIG` environment variable
2. `/etc/sober/config.toml` (created by `install.sh`)
3. `./config.toml` in the current working directory

The first file found wins. If none is found, only environment variables and compiled defaults
are used.

---

## config.toml Structure

Below is a minimal production `config.toml`. Every key maps directly to a `SOBER_*` environment
variable (documented in the [full reference](#environment-variable-reference) below).

```toml
environment = "production"

[database]
url = "postgres://sober:secret@localhost:5432/sober"
max_connections = 10

[qdrant]
url = "http://localhost:6334"
# api_key = ""  # Optional; set if Qdrant requires authentication

[llm]
base_url = "https://openrouter.ai/api/v1"
api_key = "sk-or-..."
model = "anthropic/claude-sonnet-4"
max_tokens = 4096
embedding_model = "text-embedding-3-small"
embedding_dim = 1536

[server]
host = "0.0.0.0"
port = 3000
rate_limit_max_requests = 1200
rate_limit_window_secs = 60

[auth]
session_secret = "change-me-in-production-must-be-long-and-random"
session_ttl_seconds = 2592000  # 30 days

[searxng]
url = "http://localhost:8080"

[agent]
socket_path = "/run/sober/agent.sock"
metrics_port = 9100
workspace_root = "/var/lib/sober/workspaces"
sandbox_profile = "standard"

[scheduler]
tick_interval_secs = 1
agent_socket_path = "/run/sober/agent.sock"
socket_path = "/run/sober/scheduler.sock"
max_concurrent_jobs = 10
metrics_port = 9101
workspace_root = "/var/lib/sober/workspaces"
sandbox_profile = "standard"

[web]
host = "0.0.0.0"
port = 8080
api_upstream_url = "http://localhost:3000"
# static_dir = ""  # Omit to use embedded frontend

[mcp]
request_timeout_secs = 30
max_consecutive_failures = 3
idle_timeout_secs = 300

[memory]
decay_half_life_days = 30
retrieval_boost = 0.2
prune_threshold = 0.1

[crypto]
# master_encryption_key = ""  # 256-bit hex key; generated on first run if omitted

[acp]
# command = ""   # Path to local coding agent binary (e.g. claude, goose)
# name = ""      # Display name shown in the UI
# args = "acp"   # CLI args to put the agent into ACP mode
```

---

## CLI Helpers

The `sober` CLI provides three commands for managing configuration:

```bash
# Print a fully commented config.toml template to stdout
sober config generate > config.toml

# Show the resolved configuration (merges all layers, redacts secrets)
sober config show

# Validate the resolved configuration and report errors
sober config validate
```

`sober config show` is useful for debugging — it shows exactly which value came from which
source and redacts sensitive fields like `api_key` and `session_secret`.

---

## Validation

Sõber validates configuration at startup and exits immediately on invalid values. The following
fields are always validated:

| Field | Rule |
|-------|------|
| `database.url` | Must be non-empty |
| `auth.session_secret` | Required in `production` environment |
| `crypto.master_encryption_key` | Must be valid 64-character hex string if set |

In `development` mode, missing `auth.session_secret` generates a warning but does not fail
startup.

---

## Environment Variable Reference

All `SOBER_*` variables map directly to TOML keys. Setting an environment variable always
overrides the corresponding TOML key.

### Core

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `SOBER_ENV` | `environment` | `development` | Environment mode (`development`, `production`) |
| `SOBER_CONFIG` | — | — | Explicit path to `config.toml`; skips discovery |
| `RUST_LOG` | — | `info` | Log level; not stored in `config.toml` |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | — | — | OpenTelemetry collector endpoint; not in `config.toml` |

### Database

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `SOBER_DATABASE_URL` | `database.url` | — **(required)** | PostgreSQL connection string |
| `SOBER_DATABASE_MAX_CONNECTIONS` | `database.max_connections` | `10` | Connection pool size |

### Qdrant (Vector Store)

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `SOBER_QDRANT_URL` | `qdrant.url` | `http://localhost:6334` | Qdrant gRPC endpoint |
| `SOBER_QDRANT_API_KEY` | `qdrant.api_key` | — | Qdrant API key (optional) |

### LLM Provider

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `SOBER_LLM_BASE_URL` | `llm.base_url` | `https://openrouter.ai/api/v1` | OpenAI-compatible base URL |
| `SOBER_LLM_API_KEY` | `llm.api_key` | — **(required)** | LLM provider API key |
| `SOBER_LLM_MODEL` | `llm.model` | `anthropic/claude-sonnet-4` | Chat model identifier |
| `SOBER_LLM_MAX_TOKENS` | `llm.max_tokens` | `4096` | Max completion tokens per request |
| `SOBER_LLM_EMBEDDING_MODEL` | `llm.embedding_model` | `text-embedding-3-small` | Embedding model identifier |
| `SOBER_LLM_EMBEDDING_DIM` | `llm.embedding_dim` | `1536` | Embedding vector dimensions |

### SearXNG

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `SOBER_SEARXNG_URL` | `searxng.url` | `http://localhost:8080` | SearXNG instance URL (required for `web_search` tool) |

See [Search Setup](search-setup.md) for installation and configuration details.

### Server (sober-api)

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `SOBER_SERVER_HOST` | `server.host` | `0.0.0.0` | API server bind address |
| `SOBER_SERVER_PORT` | `server.port` | `3000` | API server bind port |
| `SOBER_SERVER_RATE_LIMIT_MAX_REQUESTS` | `server.rate_limit_max_requests` | `1200` | Max requests per window |
| `SOBER_SERVER_RATE_LIMIT_WINDOW_SECS` | `server.rate_limit_window_secs` | `60` | Rate limit window in seconds |

### Auth

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `SOBER_AUTH_SESSION_SECRET` | `auth.session_secret` | — **(required in prod)** | Secret key for signing sessions |
| `SOBER_AUTH_SESSION_TTL_SECONDS` | `auth.session_ttl_seconds` | `2592000` | Session lifetime in seconds (30 days) |

### Agent (sober-agent)

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `SOBER_AGENT_SOCKET_PATH` | `agent.socket_path` | `/run/sober/agent.sock` | Agent gRPC Unix socket path |
| `SOBER_AGENT_METRICS_PORT` | `agent.metrics_port` | `9100` | Prometheus metrics port |
| `SOBER_AGENT_WORKSPACE_ROOT` | `agent.workspace_root` | `/var/lib/sober/workspaces` | Root directory for workspaces |
| `SOBER_AGENT_SANDBOX_PROFILE` | `agent.sandbox_profile` | `standard` | Sandbox policy profile name |

### Scheduler (sober-scheduler)

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `SOBER_SCHEDULER_TICK_INTERVAL_SECS` | `scheduler.tick_interval_secs` | `1` | How often the scheduler ticks |
| `SOBER_SCHEDULER_AGENT_SOCKET_PATH` | `scheduler.agent_socket_path` | `/run/sober/agent.sock` | Agent gRPC socket (for job dispatch) |
| `SOBER_SCHEDULER_SOCKET_PATH` | `scheduler.socket_path` | `/run/sober/scheduler.sock` | Scheduler Unix admin socket |
| `SOBER_SCHEDULER_MAX_CONCURRENT_JOBS` | `scheduler.max_concurrent_jobs` | `10` | Max jobs running simultaneously |
| `SOBER_SCHEDULER_METRICS_PORT` | `scheduler.metrics_port` | `9101` | Prometheus metrics port |
| `SOBER_SCHEDULER_WORKSPACE_ROOT` | `scheduler.workspace_root` | `/var/lib/sober/workspaces` | Workspace root for artifact jobs |
| `SOBER_SCHEDULER_SANDBOX_PROFILE` | `scheduler.sandbox_profile` | `standard` | Sandbox policy for artifact jobs |

### Web (sober-web)

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `SOBER_WEB_HOST` | `web.host` | `0.0.0.0` | Web server bind address |
| `SOBER_WEB_PORT` | `web.port` | `8080` | Web server bind port |
| `SOBER_WEB_API_UPSTREAM_URL` | `web.api_upstream_url` | `http://localhost:3000` | URL to proxy `/api/*` requests to |
| `SOBER_WEB_STATIC_DIR` | `web.static_dir` | — | Serve frontend from disk instead of embedded |

### MCP (Model Context Protocol)

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `SOBER_MCP_REQUEST_TIMEOUT_SECS` | `mcp.request_timeout_secs` | `30` | MCP request timeout |
| `SOBER_MCP_MAX_CONSECUTIVE_FAILURES` | `mcp.max_consecutive_failures` | `3` | Failures before marking server unhealthy |
| `SOBER_MCP_IDLE_TIMEOUT_SECS` | `mcp.idle_timeout_secs` | `300` | Idle connection timeout (5 minutes) |

### Memory

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `SOBER_MEMORY_DECAY_HALF_LIFE_DAYS` | `memory.decay_half_life_days` | `30` | Half-life for memory importance decay |
| `SOBER_MEMORY_RETRIEVAL_BOOST` | `memory.retrieval_boost` | `0.2` | Importance boost applied on retrieval |
| `SOBER_MEMORY_PRUNE_THRESHOLD` | `memory.prune_threshold` | `0.1` | Minimum importance score before pruning |

### Crypto

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `SOBER_CRYPTO_MASTER_ENCRYPTION_KEY` | `crypto.master_encryption_key` | — | 64-char hex (256-bit) master key for envelope encryption |

### ACP (Agent Client Protocol)

ACP enables Sõber to dispatch prompts through a local coding agent (e.g. Claude Code, Kimi
Code, Goose) instead of — or in addition to — direct LLM API calls.

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `SOBER_ACP_COMMAND` | `acp.command` | — | Path or name of the local agent binary |
| `SOBER_ACP_NAME` | `acp.name` | — | Display name shown in the Sõber UI |
| `SOBER_ACP_ARGS` | `acp.args` | `acp` | CLI argument(s) to enable ACP mode |

### Frontend

| Variable | TOML Key | Default | Description |
|----------|----------|---------|-------------|
| `PUBLIC_API_URL` | — | `http://localhost:3000` | API base URL used by the SvelteKit frontend |

---

## Security Notes

- **Never commit secrets** to version control. Use environment variables or a secrets manager
  for `llm.api_key`, `auth.session_secret`, and `crypto.master_encryption_key` in production.
- `auth.session_secret` should be a long, random string (at least 32 bytes). Generate one with:
  ```bash
  openssl rand -hex 32
  ```
- `crypto.master_encryption_key` must be exactly 64 hex characters (256 bits). Generate with:
  ```bash
  openssl rand -hex 32
  ```
- If `crypto.master_encryption_key` is not set, secrets are not encrypted at rest.
  This is acceptable in development but should not be used in production.

---

## Next Step

With configuration in place, continue to [First Run](first-run.md).
