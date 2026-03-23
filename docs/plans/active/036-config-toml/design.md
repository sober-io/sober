# Full Configuration Support — config.toml + Env Override

## Overview

Replace the manual env-var-only config loading with a layered configuration system where everything can be defined in `config.toml` but any value can be overridden by environment variables. Env vars always win.

## Current State

- `AppConfig::load_from_env()` reads config exclusively from env vars via a custom `EnvSource` helper (`backend/crates/sober-core/src/config.rs`)
- `dotenvy` loads `.env` files in development
- `toml = "0.8"` is already a workspace dependency (used by `WorkspaceConfig`, `PluginManifest`, `SandboxConfig`)
- All `AppConfig` nested structs already derive `serde::Deserialize`
- `install.sh` writes `/etc/sober/config.toml` and `/etc/sober/.env` — but the TOML file is never read by the application
- `infra/config/config.toml.example` exists but is incomplete/outdated (only covers server, storage, database, qdrant, logging)
- All four binaries (sober-api, sober-agent, sober-scheduler, sober-web) call `AppConfig::load_from_env()`

## Design

### Layered Config Resolution

Priority (highest wins):

```
Environment variables
  └── .env file (development only, via dotenvy)
       └── config.toml file
            └── Compiled defaults
```

### Config File Discovery

Search order (first found wins):
1. `SOBER_CONFIG` env var (explicit path)
2. `/etc/sober/config.toml` (production — where install.sh puts it)
3. `./config.toml` (development — project root)

If no file is found, fall back to env vars + defaults (current behavior). This ensures backwards compatibility.

### Implementation Approach

Use `toml::from_str()` to deserialize the TOML file into `AppConfig`, then overlay env vars on top. No new dependencies needed — `toml` and `serde` are already present.

1. Add `#[serde(default)]` to all `AppConfig` nested structs so partial TOML files work
2. Add a `Default` impl for each config struct with the same defaults currently hardcoded in `EnvSource` calls
3. New `AppConfig::load()` method:
   - Discover and read config.toml (if exists)
   - Deserialize via `toml::from_str::<AppConfig>()`
   - Overlay env vars on top (any set env var wins over TOML value)
   - Validate required fields (e.g., `DATABASE_URL`)
4. Deprecate `AppConfig::load_from_env()` — keep it as a thin wrapper calling `load()` for backwards compat

### TOML Structure

Full config.toml matching all `AppConfig` sections:

```toml
environment = "production"  # or "development"

[database]
url = "postgres://sober:password@localhost/sober"
max_connections = 10

[qdrant]
url = "http://localhost:6334"
# api_key = "optional"

[llm]
base_url = "https://openrouter.ai/api/v1"
api_key = "sk-..."
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
session_secret = "change-me-in-production"
session_ttl_seconds = 2592000

[searxng]
url = "http://localhost:8080"

[admin]
socket_path = "/run/sober/admin.sock"

[scheduler]
tick_interval_secs = 60
agent_socket_path = "/run/sober/agent.sock"
socket_path = "/run/sober/scheduler.sock"
max_concurrent_jobs = 10

[mcp]
request_timeout_secs = 30
max_consecutive_failures = 3
idle_timeout_secs = 300

[memory]
decay_half_life_days = 30
retrieval_boost = 0.2
prune_threshold = 0.1

[crypto]
# master_encryption_key = "hex-encoded-256-bit-key"

# Optional: local coding agent integration
[acp_agent]
command = "claude"
name = "Claude Code"
args = ["acp"]
```

### Env Var Mapping

Each TOML key maps to a flat env var. The mapping is:

| TOML path | Env var |
|-----------|---------|
| `database.url` | `DATABASE_URL` |
| `database.max_connections` | `DATABASE_MAX_CONNECTIONS` |
| `llm.base_url` | `LLM_BASE_URL` |
| `llm.api_key` | `LLM_API_KEY` |
| `server.port` | `PORT` |
| ... | (existing env var names unchanged) |

Existing env var names are preserved for backwards compatibility. The overlay reads each known env var and patches the deserialized struct.

### Validation

- `DATABASE_URL` is required (must come from TOML or env)
- `SESSION_SECRET` required when `environment = "production"`
- `MASTER_ENCRYPTION_KEY` must be valid hex if provided
- Fail fast with clear error messages naming the missing field and both config sources

### Changes to install.sh

- Update `write_default_config()` to write a complete `config.toml` matching all `AppConfig` sections
- Update `collect_config()` to write user-provided values (DATABASE_URL, LLM_*) into the TOML file instead of `.env`
- Keep `.env` support for backwards compat but prefer TOML for new installs
- Update `infra/config/config.toml.example` to match the full structure

### Testing

- Unit tests: TOML-only loading, env-only loading, env overrides TOML, partial TOML with defaults
- Existing integration tests continue to work (they use env vars, which still win)
- Test config file discovery order

## Scope

**In scope:**
- Layered config loading (TOML + env overlay)
- Full `config.toml` format covering all `AppConfig` sections
- Updated `config.toml.example`
- Updated `install.sh` to write complete TOML
- `SOBER_CONFIG` env var for custom config path
- Backwards compatibility with env-only setups

**Out of scope:**
- Hot-reloading config changes (restart required)
- Config validation CLI command (could be a follow-up)
- YAML/JSON config formats
