# #036: config.toml + Env Override — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace env-var-only config loading with layered TOML + env override system, migrate all env vars to `SOBER_*` prefix.

**Architecture:** `AppConfig` gains `#[derive(Deserialize)]` + `Default` impls. New `AppConfig::load()` discovers a TOML file, deserializes it, overlays `SOBER_*` env vars, then validates — returns `Result<AppConfig>`. A separate `LoadedConfig::load()` does the same but also tracks per-field provenance (default/toml/env) — used only by CLI commands.

**Tech Stack:** Rust, toml 0.8, serde (already workspace deps). No new dependencies.

**Design spec:** `docs/plans/pending/036-config-toml/design.md`

---

## File Map

| File | Action | Purpose |
|------|--------|---------|
| `backend/crates/sober-core/src/config.rs` | Major rewrite | Deserialize, Default, load(), overlay, validate, source tracking |
| `backend/crates/sober-core/src/lib.rs` | Minor | Re-export `LoadedConfig` |
| `backend/crates/sober-api/src/main.rs` | Small | `load_from_env()` → `load()` |
| `backend/crates/sober-agent/src/main.rs` | Moderate | `load()` + use `config.agent_process.*` |
| `backend/crates/sober-agent/src/agent.rs` | Small | Store + use `workspace_root` field |
| `backend/crates/sober-scheduler/src/main.rs` | Moderate | `load()` + use new scheduler fields |
| `backend/crates/sober-web/src/main.rs` | Moderate | Add `AppConfig::load()`, use `config.web.*` |
| `backend/crates/sober-cli/src/cli.rs` | Small | Add `ConfigCommand::Generate` |
| `backend/crates/sober-cli/src/commands/config.rs` | Major | Enhance show/validate, add generate |
| `backend/crates/sober-cli/src/sober.rs` | Small | `connect_db()` uses AppConfig |
| `.env.example` | Rewrite | All vars → `SOBER_*` prefix |
| `docker-compose.yml` | Major | All env vars → `SOBER_*` prefix |
| `infra/config/config.toml.example` | Rewrite | Full structure with comments |
| `scripts/install.sh` | Major | Write complete TOML, stop creating `.env` |

**Naming note:** `sober-agent/src/agent.rs` already has an `AgentConfig` struct (behavioral:
`max_tool_iterations`, `context_token_budget`). The new process config in `sober-core` is named
`AgentProcessConfig` to avoid collision. TOML section stays `[agent]` via `#[serde(rename)]`.

---

### Task 0: Move plan to active

- [ ] **Step 1: Move plan folder from pending to active**

```bash
git mv docs/plans/pending/036-config-toml docs/plans/active/036-config-toml
git add docs/plans/
git commit -m "docs: move plan 036 to active"
```

---

### Task 1: Add Deserialize + Default to Environment enum

**Files:**
- Modify: `backend/crates/sober-core/src/config.rs:46-53`
- Test: `backend/crates/sober-core/src/config.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write test for Environment deserialization**

In the `#[cfg(test)]` module at the bottom of `config.rs`, add:

```rust
#[test]
fn environment_deserializes_from_lowercase() {
    #[derive(Deserialize)]
    struct Wrapper {
        environment: Environment,
    }
    let toml_str = r#"environment = "production""#;
    let w: Wrapper = toml::from_str(toml_str).unwrap();
    assert_eq!(w.environment, Environment::Production);

    let toml_str = r#"environment = "development""#;
    let w: Wrapper = toml::from_str(toml_str).unwrap();
    assert_eq!(w.environment, Environment::Development);
}

#[test]
fn environment_defaults_to_development() {
    assert_eq!(Environment::default(), Environment::Development);
}
```

Add `use serde::Deserialize;` to the test module imports (for the `Wrapper` derive).
Add `use toml;` to the test module imports.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-core -q -- config::tests::environment_deserializes 2>&1 | tail -5`

Expected: compile error — `Environment` doesn't implement `Deserialize` or `Default`.

- [ ] **Step 3: Add Deserialize + Default to Environment**

Change the `Environment` enum:

```rust
/// Runtime environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    /// Local development with pretty logging.
    Development,
    /// Production with JSON logging.
    Production,
}

impl Default for Environment {
    fn default() -> Self {
        Self::Development
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-core -q -- config::tests::environment 2>&1 | tail -5`

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-core/src/config.rs
git commit -m "feat(core): add Deserialize + Default to Environment enum"
```

---

### Task 2: Add Deserialize + Default to all existing config structs

**Files:**
- Modify: `backend/crates/sober-core/src/config.rs:55-191`

- [ ] **Step 1: Write test for TOML round-trip of DatabaseConfig**

```rust
#[test]
fn database_config_deserializes_from_toml() {
    #[derive(Deserialize)]
    struct Wrapper {
        database: DatabaseConfig,
    }
    let toml_str = r#"
[database]
url = "postgres://test:test@localhost/test"
max_connections = 20
"#;
    let w: Wrapper = toml::from_str(toml_str).unwrap();
    assert_eq!(w.database.url, "postgres://test:test@localhost/test");
    assert_eq!(w.database.max_connections, 20);
}

#[test]
fn database_config_defaults() {
    let cfg = DatabaseConfig::default();
    assert_eq!(cfg.url, "");
    assert_eq!(cfg.max_connections, 10);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-core -q -- config::tests::database_config 2>&1 | tail -5`

Expected: compile errors.

- [ ] **Step 3: Add Deserialize + Default to all config structs**

Add `#[derive(serde::Deserialize)]` and `#[serde(default)]` to every config struct. Add
`Default` impl for each with these values (matching current `EnvSource` defaults):

| Struct | Field | Default |
|--------|-------|---------|
| `DatabaseConfig` | `url` | `""` (empty — validated later) |
| `DatabaseConfig` | `max_connections` | `10` |
| `QdrantConfig` | `url` | `"http://localhost:6334"` |
| `QdrantConfig` | `api_key` | `None` |
| `LlmConfig` | `base_url` | `"https://openrouter.ai/api/v1"` |
| `LlmConfig` | `api_key` | `None` |
| `LlmConfig` | `model` | `"anthropic/claude-sonnet-4"` |
| `LlmConfig` | `max_tokens` | `4096` |
| `LlmConfig` | `embedding_model` | `"text-embedding-3-small"` |
| `LlmConfig` | `embedding_dim` | `1536` |
| `ServerConfig` | `host` | `"0.0.0.0"` |
| `ServerConfig` | `port` | `3000` |
| `ServerConfig` | `rate_limit_max_requests` | `1200` |
| `ServerConfig` | `rate_limit_window_secs` | `60` |
| `AuthConfig` | `session_secret` | `None` |
| `AuthConfig` | `session_ttl_seconds` | `2_592_000` |
| `SearxngConfig` | `url` | `"http://localhost:8080"` |
| `AdminConfig` | `socket_path` | `PathBuf::from("/run/sober/admin.sock")` |
| `SchedulerConfig` | `tick_interval_secs` | `1` |
| `SchedulerConfig` | `agent_socket_path` | `PathBuf::from("/run/sober/agent.sock")` |
| `SchedulerConfig` | `socket_path` | `PathBuf::from("/run/sober/scheduler.sock")` |
| `SchedulerConfig` | `max_concurrent_jobs` | `10` |
| `McpConfig` | `request_timeout_secs` | `30` |
| `McpConfig` | `max_consecutive_failures` | `3` |
| `McpConfig` | `idle_timeout_secs` | `300` |
| `AcpAgentConfig` | `name` | `""` (empty) |
| `AcpAgentConfig` | `command` | `""` (empty) |
| `AcpAgentConfig` | `args` | `vec!["acp".to_owned()]` |
| `MemoryConfig` | `decay_half_life_days` | `30` |
| `MemoryConfig` | `retrieval_boost` | `0.2` |
| `MemoryConfig` | `prune_threshold` | `0.1` |
| `CryptoConfig` | `master_encryption_key` | `None` |

For `AppConfig` itself, also add `#[derive(serde::Deserialize)]` and `#[serde(default)]`.
The `acp` field uses `Option<AcpAgentConfig>` — absent TOML section = `None`.

`AppConfig::default()` composes all sub-struct defaults plus `acp: None` and
`environment: Environment::default()`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-core -q -- config::tests 2>&1 | tail -10`

Expected: all tests pass (including existing ones — `load_from` still works).

- [ ] **Step 5: Run clippy**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo clippy -p sober-core -q -- -D warnings 2>&1 | tail -5`

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-core/src/config.rs
git commit -m "feat(core): add Deserialize + Default to all config structs"
```

---

### Task 3: Add new config structs (AgentProcessConfig, WebConfig, SchedulerConfig fields)

**Files:**
- Modify: `backend/crates/sober-core/src/config.rs`

- [ ] **Step 1: Write tests for new structs**

```rust
#[test]
fn agent_process_config_defaults() {
    let cfg = AgentProcessConfig::default();
    assert_eq!(cfg.socket_path, PathBuf::from("/run/sober/agent.sock"));
    assert_eq!(cfg.metrics_port, 9100);
    assert_eq!(cfg.workspace_root, PathBuf::from("/var/lib/sober/workspaces"));
    assert_eq!(cfg.sandbox_profile, "standard");
}

#[test]
fn web_config_defaults() {
    let cfg = WebConfig::default();
    assert_eq!(cfg.host, "0.0.0.0");
    assert_eq!(cfg.port, 8080);
    assert_eq!(cfg.api_upstream_url, "http://localhost:3000");
    assert!(cfg.static_dir.is_none());
}

#[test]
fn scheduler_config_has_new_fields() {
    let cfg = SchedulerConfig::default();
    assert_eq!(cfg.metrics_port, 9101);
    assert_eq!(cfg.workspace_root, PathBuf::from("/var/lib/sober/workspaces"));
    assert_eq!(cfg.sandbox_profile, "standard");
}

#[test]
fn full_toml_deserializes() {
    let toml_str = r#"
environment = "production"

[database]
url = "postgres://test:test@localhost/test"

[agent]
socket_path = "/tmp/agent.sock"
metrics_port = 9200

[web]
port = 9090
api_upstream_url = "http://api:3000"

[scheduler]
metrics_port = 9201
"#;
    let config: AppConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.environment, Environment::Production);
    assert_eq!(config.agent.socket_path, PathBuf::from("/tmp/agent.sock"));
    assert_eq!(config.agent.metrics_port, 9200);
    assert_eq!(config.web.port, 9090);
    assert_eq!(config.scheduler.metrics_port, 9201);
    // Non-specified fields use defaults
    assert_eq!(config.server.port, 3000);
    assert!(config.acp.is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-core -q -- config::tests::agent_process 2>&1 | tail -5`

Expected: compile error — types don't exist.

- [ ] **Step 3: Add AgentProcessConfig struct**

```rust
/// Agent process settings (sober-agent binary).
///
/// Configurable via `[agent]` TOML section or `SOBER_AGENT_*` env vars.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct AgentProcessConfig {
    /// Path to the agent gRPC socket.
    pub socket_path: PathBuf,
    /// Prometheus metrics port.
    pub metrics_port: u16,
    /// Root directory for workspaces.
    pub workspace_root: PathBuf,
    /// Sandbox profile name.
    pub sandbox_profile: String,
}

impl Default for AgentProcessConfig {
    fn default() -> Self {
        Self {
            socket_path: PathBuf::from("/run/sober/agent.sock"),
            metrics_port: 9100,
            workspace_root: PathBuf::from("/var/lib/sober/workspaces"),
            sandbox_profile: "standard".to_owned(),
        }
    }
}
```

- [ ] **Step 4: Add WebConfig struct**

```rust
/// Web reverse-proxy settings (sober-web binary).
///
/// Configurable via `[web]` TOML section or `SOBER_WEB_*` env vars.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct WebConfig {
    /// Bind address.
    pub host: String,
    /// Bind port.
    pub port: u16,
    /// Upstream API server URL to proxy `/api/*` and WebSocket.
    pub api_upstream_url: String,
    /// Optional path to serve static files from disk instead of embedded.
    pub static_dir: Option<PathBuf>,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_owned(),
            port: 8080,
            api_upstream_url: "http://localhost:3000".to_owned(),
            static_dir: None,
        }
    }
}
```

- [ ] **Step 5: Add new fields to SchedulerConfig**

Add to `SchedulerConfig`:

```rust
/// Prometheus metrics port.
pub metrics_port: u16,
/// Root directory for workspaces.
pub workspace_root: PathBuf,
/// Sandbox profile name.
pub sandbox_profile: String,
```

Update `SchedulerConfig::default()` to include:
- `metrics_port: 9101`
- `workspace_root: PathBuf::from("/var/lib/sober/workspaces")`
- `sandbox_profile: "standard".to_owned()`

- [ ] **Step 6: Add new fields to AppConfig**

Add to `AppConfig`:

```rust
/// Agent process settings (socket, metrics, workspace).
#[serde(rename = "agent")]
pub agent: AgentProcessConfig,
/// Web reverse-proxy settings.
pub web: WebConfig,
```

Update `AppConfig::default()` to include `agent: AgentProcessConfig::default()` and
`web: WebConfig::default()`.

Update `AppConfig::load_from()` to also set these fields (use defaults — existing env vars
are not read yet, that happens in the overlay later).

- [ ] **Step 7: Run all tests**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-core -q 2>&1 | tail -10`

Expected: all pass.

- [ ] **Step 8: Build workspace to check downstream compilation**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo build -q 2>&1 | tail -20`

Downstream crates (sober-api, sober-agent, etc.) may fail to compile if they destructure
`AppConfig` exhaustively. Fix any compilation errors by adding the new fields where needed.

- [ ] **Step 9: Commit**

```bash
git add backend/crates/sober-core/src/config.rs
git commit -m "feat(core): add AgentProcessConfig, WebConfig, SchedulerConfig fields"
```

---

### Task 4: Implement config file discovery + TOML loading

**Files:**
- Modify: `backend/crates/sober-core/src/config.rs`

- [ ] **Step 1: Write tests for config file discovery**

```rust
#[test]
fn discover_config_returns_none_when_no_file() {
    // With no SOBER_CONFIG set and non-existent paths, returns None
    let path = AppConfig::discover_config_path(|_| None, |p| p == std::path::Path::new("/nonexistent"));
    assert!(path.is_none());
}

#[test]
fn discover_config_prefers_sober_config_env() {
    let path = AppConfig::discover_config_path(
        |key| if key == "SOBER_CONFIG" { Some("/custom/config.toml".to_owned()) } else { None },
        |_| true, // all paths "exist"
    );
    assert_eq!(path, Some(PathBuf::from("/custom/config.toml")));
}

#[test]
fn discover_config_falls_through_to_etc() {
    let path = AppConfig::discover_config_path(
        |_| None, // no SOBER_CONFIG
        |p| p == std::path::Path::new("/etc/sober/config.toml"),
    );
    assert_eq!(path, Some(PathBuf::from("/etc/sober/config.toml")));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-core -q -- config::tests::discover 2>&1 | tail -5`

Expected: compile error — `discover_config_path` doesn't exist.

- [ ] **Step 3: Implement discover_config_path**

```rust
impl AppConfig {
    /// Discovers the config file path using the search order:
    /// 1. `SOBER_CONFIG` env var
    /// 2. `/etc/sober/config.toml`
    /// 3. `./config.toml`
    fn discover_config_path(
        get_env: impl Fn(&str) -> Option<String>,
        path_exists: impl Fn(&std::path::Path) -> bool,
    ) -> Option<PathBuf> {
        // 1. Explicit path via env var
        if let Some(path) = get_env("SOBER_CONFIG") {
            return Some(PathBuf::from(path));
        }
        // 2. System config
        let etc = std::path::Path::new("/etc/sober/config.toml");
        if path_exists(etc) {
            return Some(etc.to_path_buf());
        }
        // 3. Local config
        let local = std::path::Path::new("./config.toml");
        if path_exists(local) {
            return Some(local.to_path_buf());
        }
        None
    }
}
```

- [ ] **Step 4: Write test for TOML loading**

```rust
#[test]
fn load_from_toml_string() {
    let toml_str = r#"
[database]
url = "postgres://test:test@localhost/test"

[server]
port = 4000
"#;
    let config: AppConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.database.url, "postgres://test:test@localhost/test");
    assert_eq!(config.server.port, 4000);
    // Defaults for unspecified fields
    assert_eq!(config.llm.model, "anthropic/claude-sonnet-4");
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-core -q -- config::tests 2>&1 | tail -10`

Expected: all pass.

- [ ] **Step 6: Implement load_resolved() (internal) and load() / load_unvalidated()**

The internal `load_resolved()` does discovery + TOML + overlay without validation.
`load()` calls it + `validate()`. `load_unvalidated()` skips validation — used by
`sober-web` (which doesn't need a database) and `sober config show/generate`.

```rust
impl AppConfig {
    /// Internal: discover TOML + overlay env vars, no validation.
    fn load_resolved() -> Result<Self, AppError> {
        dotenvy::dotenv().ok();

        let config_path = Self::discover_config_path(
            |key| std::env::var(key).ok(),
            |p| p.exists(),
        );

        let mut config = match &config_path {
            Some(path) => {
                let contents = std::fs::read_to_string(path).map_err(|e| {
                    AppError::Validation(format!(
                        "failed to read config file {}: {e}",
                        path.display()
                    ))
                })?;
                toml::from_str(&contents).map_err(|e| {
                    AppError::Validation(format!(
                        "failed to parse config file {}: {e}",
                        path.display()
                    ))
                })?
            }
            None => AppConfig::default(),
        };

        config.apply_env_overrides(|key| std::env::var(key).ok());
        Ok(config)
    }

    /// Loads configuration from TOML file + env var overlay, then validates.
    ///
    /// Use this for binaries that need a fully validated config (api, agent, scheduler).
    pub fn load() -> Result<Self, AppError> {
        let config = Self::load_resolved()?;
        config.validate()?;
        Ok(config)
    }

    /// Loads configuration without validation.
    ///
    /// Use for binaries that don't need all config sections (e.g., sober-web only needs
    /// `[web]`) or CLI commands that display config without requiring it to be complete.
    pub fn load_unvalidated() -> Result<Self, AppError> {
        Self::load_resolved()
    }
}
```

- [ ] **Step 7: Run clippy + tests**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo clippy -p sober-core -q -- -D warnings && cargo test -p sober-core -q 2>&1 | tail -10`

- [ ] **Step 8: Commit**

```bash
git add backend/crates/sober-core/src/config.rs
git commit -m "feat(core): implement config file discovery and TOML loading"
```

---

### Task 5: Implement env var overlay

**Files:**
- Modify: `backend/crates/sober-core/src/config.rs`

- [ ] **Step 1: Write test for env overlay**

```rust
#[test]
fn env_overrides_toml_values() {
    let toml_str = r#"
[database]
url = "postgres://toml@localhost/db"

[server]
port = 4000
"#;
    let mut config: AppConfig = toml::from_str(toml_str).unwrap();

    config.apply_env_overrides(env_from(&[
        ("SOBER_DATABASE_URL", "postgres://env@localhost/db"),
        ("SOBER_SERVER_PORT", "5000"),
    ]));

    assert_eq!(config.database.url, "postgres://env@localhost/db");
    assert_eq!(config.server.port, 5000);
}

#[test]
fn env_overlay_creates_acp_from_none() {
    let mut config = AppConfig::default();
    assert!(config.acp.is_none());

    config.apply_env_overrides(env_from(&[
        ("SOBER_ACP_COMMAND", "claude"),
        ("SOBER_ACP_NAME", "Claude Code"),
    ]));

    let acp = config.acp.as_ref().unwrap();
    assert_eq!(acp.command, "claude");
    assert_eq!(acp.name, "Claude Code");
    assert_eq!(acp.args, vec!["acp"]);
}

#[test]
fn env_overlay_acp_args_whitespace_split() {
    let mut config = AppConfig::default();
    config.apply_env_overrides(env_from(&[
        ("SOBER_ACP_COMMAND", "claude"),
        ("SOBER_ACP_ARGS", "acp --verbose"),
    ]));
    let acp = config.acp.as_ref().unwrap();
    assert_eq!(acp.args, vec!["acp", "--verbose"]);
}

#[test]
fn env_does_not_override_unset_vars() {
    let toml_str = r#"
[database]
url = "postgres://toml@localhost/db"
"#;
    let mut config: AppConfig = toml::from_str(toml_str).unwrap();
    config.apply_env_overrides(env_from(&[]));
    assert_eq!(config.database.url, "postgres://toml@localhost/db");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-core -q -- config::tests::env_overrides 2>&1 | tail -5`

Expected: compile error — `apply_env_overrides` doesn't exist.

- [ ] **Step 3: Implement apply_env_overrides**

Hand-written per-field overlay method. Uses a helper to parse typed values:

```rust
impl AppConfig {
    /// Overlays environment variables onto the config, overwriting any TOML or default values.
    pub(crate) fn apply_env_overrides(&mut self, get_var: impl Fn(&str) -> Option<String>) {
        let env = EnvSource(&get_var);

        // environment
        if let Some(val) = get_var("SOBER_ENV") {
            self.environment = match val.as_str() {
                "production" => Environment::Production,
                _ => Environment::Development,
            };
        }

        // database
        if let Some(v) = get_var("SOBER_DATABASE_URL") { self.database.url = v; }
        if let Ok(v) = env.parse_opt("SOBER_DATABASE_MAX_CONNECTIONS") { if let Some(v) = v { self.database.max_connections = v; } }

        // qdrant
        if let Some(v) = get_var("SOBER_QDRANT_URL") { self.qdrant.url = v; }
        if let Some(v) = get_var("SOBER_QDRANT_API_KEY") { self.qdrant.api_key = Some(v); }

        // llm
        if let Some(v) = get_var("SOBER_LLM_BASE_URL") { self.llm.base_url = v; }
        if let Some(v) = get_var("SOBER_LLM_API_KEY") { self.llm.api_key = Some(v); }
        if let Some(v) = get_var("SOBER_LLM_MODEL") { self.llm.model = v; }
        if let Ok(v) = env.parse_opt("SOBER_LLM_MAX_TOKENS") { if let Some(v) = v { self.llm.max_tokens = v; } }
        if let Some(v) = get_var("SOBER_LLM_EMBEDDING_MODEL") { self.llm.embedding_model = v; }
        if let Ok(v) = env.parse_opt("SOBER_LLM_EMBEDDING_DIM") { if let Some(v) = v { self.llm.embedding_dim = v; } }

        // server
        if let Some(v) = get_var("SOBER_SERVER_HOST") { self.server.host = v; }
        if let Ok(v) = env.parse_opt("SOBER_SERVER_PORT") { if let Some(v) = v { self.server.port = v; } }
        if let Ok(v) = env.parse_opt("SOBER_SERVER_RATE_LIMIT_MAX_REQUESTS") { if let Some(v) = v { self.server.rate_limit_max_requests = v; } }
        if let Ok(v) = env.parse_opt("SOBER_SERVER_RATE_LIMIT_WINDOW_SECS") { if let Some(v) = v { self.server.rate_limit_window_secs = v; } }

        // auth
        if let Some(v) = get_var("SOBER_AUTH_SESSION_SECRET") { self.auth.session_secret = Some(v); }
        if let Ok(v) = env.parse_opt("SOBER_AUTH_SESSION_TTL_SECONDS") { if let Some(v) = v { self.auth.session_ttl_seconds = v; } }

        // searxng
        if let Some(v) = get_var("SOBER_SEARXNG_URL") { self.searxng.url = v; }

        // admin
        if let Some(v) = get_var("SOBER_ADMIN_SOCKET_PATH") { self.admin.socket_path = PathBuf::from(v); }

        // agent (process config)
        if let Some(v) = get_var("SOBER_AGENT_SOCKET_PATH") { self.agent.socket_path = PathBuf::from(v); }
        if let Ok(v) = env.parse_opt("SOBER_AGENT_METRICS_PORT") { if let Some(v) = v { self.agent.metrics_port = v; } }
        if let Some(v) = get_var("SOBER_AGENT_WORKSPACE_ROOT") { self.agent.workspace_root = PathBuf::from(v); }
        if let Some(v) = get_var("SOBER_AGENT_SANDBOX_PROFILE") { self.agent.sandbox_profile = v; }

        // scheduler
        if let Ok(v) = env.parse_opt("SOBER_SCHEDULER_TICK_INTERVAL_SECS") { if let Some(v) = v { self.scheduler.tick_interval_secs = v; } }
        if let Some(v) = get_var("SOBER_SCHEDULER_AGENT_SOCKET_PATH") { self.scheduler.agent_socket_path = PathBuf::from(v); }
        if let Some(v) = get_var("SOBER_SCHEDULER_SOCKET_PATH") { self.scheduler.socket_path = PathBuf::from(v); }
        if let Ok(v) = env.parse_opt("SOBER_SCHEDULER_MAX_CONCURRENT_JOBS") { if let Some(v) = v { self.scheduler.max_concurrent_jobs = v; } }
        if let Ok(v) = env.parse_opt("SOBER_SCHEDULER_METRICS_PORT") { if let Some(v) = v { self.scheduler.metrics_port = v; } }
        if let Some(v) = get_var("SOBER_SCHEDULER_WORKSPACE_ROOT") { self.scheduler.workspace_root = PathBuf::from(v); }
        if let Some(v) = get_var("SOBER_SCHEDULER_SANDBOX_PROFILE") { self.scheduler.sandbox_profile = v; }

        // web
        if let Some(v) = get_var("SOBER_WEB_HOST") { self.web.host = v; }
        if let Ok(v) = env.parse_opt("SOBER_WEB_PORT") { if let Some(v) = v { self.web.port = v; } }
        if let Some(v) = get_var("SOBER_WEB_API_UPSTREAM_URL") { self.web.api_upstream_url = v; }
        if let Some(v) = get_var("SOBER_WEB_STATIC_DIR") { self.web.static_dir = Some(PathBuf::from(v)); }

        // mcp
        if let Ok(v) = env.parse_opt("SOBER_MCP_REQUEST_TIMEOUT_SECS") { if let Some(v) = v { self.mcp.request_timeout_secs = v; } }
        if let Ok(v) = env.parse_opt("SOBER_MCP_MAX_CONSECUTIVE_FAILURES") { if let Some(v) = v { self.mcp.max_consecutive_failures = v; } }
        if let Ok(v) = env.parse_opt("SOBER_MCP_IDLE_TIMEOUT_SECS") { if let Some(v) = v { self.mcp.idle_timeout_secs = v; } }

        // memory
        if let Ok(v) = env.parse_opt("SOBER_MEMORY_DECAY_HALF_LIFE_DAYS") { if let Some(v) = v { self.memory.decay_half_life_days = v; } }
        if let Ok(v) = env.parse_opt("SOBER_MEMORY_RETRIEVAL_BOOST") { if let Some(v) = v { self.memory.retrieval_boost = v; } }
        if let Ok(v) = env.parse_opt("SOBER_MEMORY_PRUNE_THRESHOLD") { if let Some(v) = v { self.memory.prune_threshold = v; } }

        // crypto
        if let Some(v) = get_var("SOBER_CRYPTO_MASTER_ENCRYPTION_KEY") { self.crypto.master_encryption_key = Some(v); }

        // acp (optional section — any SOBER_ACP_* env var triggers Some)
        let acp_command = get_var("SOBER_ACP_COMMAND");
        let acp_name = get_var("SOBER_ACP_NAME");
        let acp_args = get_var("SOBER_ACP_ARGS");
        if acp_command.is_some() || acp_name.is_some() || acp_args.is_some() {
            let acp = self.acp.get_or_insert_with(AcpAgentConfig::default);
            if let Some(v) = acp_command { acp.command = v; }
            if let Some(v) = acp_name { acp.name = v; }
            if let Some(v) = acp_args {
                acp.args = v.split_whitespace().map(String::from).collect();
            }
        }
    }
}
```

Also add a `parse_opt` helper to `EnvSource`:

```rust
impl<F: Fn(&str) -> Option<String>> EnvSource<F> {
    /// Parses an optional env var. Returns Ok(None) if absent, Ok(Some(v)) if valid.
    fn parse_opt<T>(&self, key: &str) -> Result<Option<T>, AppError>
    where
        T: std::str::FromStr + std::fmt::Display,
        T::Err: std::fmt::Display,
    {
        match (self.0)(key) {
            Some(val) => val.parse::<T>()
                .map(Some)
                .map_err(|e| AppError::Validation(format!("invalid value for {key}: {e} (got: {val})"))),
            None => Ok(None),
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-core -q -- config::tests 2>&1 | tail -10`

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-core/src/config.rs
git commit -m "feat(core): implement SOBER_* env var overlay"
```

---

### Task 6: Implement validation

**Files:**
- Modify: `backend/crates/sober-core/src/config.rs`

- [ ] **Step 1: Write validation tests**

```rust
#[test]
fn validate_rejects_empty_database_url() {
    let config = AppConfig::default();
    let err = config.validate().unwrap_err();
    assert!(matches!(err, AppError::Validation(msg) if msg.contains("database.url")));
}

#[test]
fn validate_rejects_missing_session_secret_in_production() {
    let mut config = AppConfig::default();
    config.database.url = "postgres://test@localhost/test".to_owned();
    config.environment = Environment::Production;
    let err = config.validate().unwrap_err();
    assert!(matches!(err, AppError::Validation(msg) if msg.contains("session_secret")));
}

#[test]
fn validate_accepts_missing_session_secret_in_development() {
    let mut config = AppConfig::default();
    config.database.url = "postgres://test@localhost/test".to_owned();
    config.environment = Environment::Development;
    assert!(config.validate().is_ok());
}

#[test]
fn validate_rejects_invalid_hex_encryption_key() {
    let mut config = AppConfig::default();
    config.database.url = "postgres://test@localhost/test".to_owned();
    config.crypto.master_encryption_key = Some("not-valid-hex".to_owned());
    let err = config.validate().unwrap_err();
    assert!(matches!(err, AppError::Validation(msg) if msg.contains("master_encryption_key")));
}

#[test]
fn validate_accepts_valid_config() {
    let mut config = AppConfig::default();
    config.database.url = "postgres://test@localhost/test".to_owned();
    assert!(config.validate().is_ok());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-core -q -- config::tests::validate 2>&1 | tail -5`

- [ ] **Step 3: Implement validate()**

```rust
impl AppConfig {
    /// Validates the loaded configuration.
    ///
    /// Called after TOML loading + env overlay. Checks required fields and format constraints.
    pub fn validate(&self) -> Result<(), AppError> {
        if self.database.url.is_empty() {
            return Err(AppError::Validation(
                "missing required config: database.url — set in config.toml [database] section or SOBER_DATABASE_URL env var".to_owned(),
            ));
        }

        if self.environment == Environment::Production && self.auth.session_secret.is_none() {
            return Err(AppError::Validation(
                "missing required config: auth.session_secret — required when environment = production. Set in config.toml [auth] section or SOBER_AUTH_SESSION_SECRET env var".to_owned(),
            ));
        }

        if let Some(ref key) = self.crypto.master_encryption_key {
            if key.len() != 64 || hex::decode(key).is_err() {
                return Err(AppError::Validation(
                    "invalid config: crypto.master_encryption_key — must be a 64-character hex string (256-bit key). Generate with: openssl rand -hex 32".to_owned(),
                ));
            }
        }

        Ok(())
    }
}
```

Note: check if the `hex` crate is available in `sober-core`. If not, use a manual hex validation:

```rust
fn is_valid_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
```

- [ ] **Step 4: Run tests**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-core -q -- config::tests 2>&1 | tail -10`

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-core/src/config.rs
git commit -m "feat(core): implement config validation"
```

---

### Task 7: Deprecate load_from_env, update load_from for tests

**Files:**
- Modify: `backend/crates/sober-core/src/config.rs`

- [ ] **Step 1: Write test that load_from still works with new env var names**

```rust
#[test]
fn load_from_with_new_env_var_names() {
    let config = AppConfig::load_from(env_from(&[
        ("SOBER_DATABASE_URL", "postgres://test:test@localhost/test"),
        ("SOBER_SERVER_PORT", "5000"),
        ("SOBER_ENV", "production"),
        ("SOBER_AUTH_SESSION_SECRET", "test-secret"),
    ]))
    .unwrap();
    assert_eq!(config.database.url, "postgres://test:test@localhost/test");
    assert_eq!(config.server.port, 5000);
    assert_eq!(config.environment, Environment::Production);
}
```

- [ ] **Step 2: Refactor load_from to use new overlay + validate pattern**

Rewrite `load_from` to:
1. Start from `AppConfig::default()`
2. Call `apply_env_overrides(get_var)`
3. Call `validate()`

```rust
/// Loads configuration from an arbitrary key-value source (for testing).
///
/// Starts from defaults, applies the provided vars as overrides, then validates.
pub fn load_from(get_var: impl Fn(&str) -> Option<String>) -> Result<Self, AppError> {
    let mut config = Self::default();
    config.apply_env_overrides(&get_var);
    config.validate()?;
    Ok(config)
}

/// Loads configuration from environment variables.
///
/// **Deprecated:** Use [`load()`](AppConfig::load) which also reads `config.toml`.
#[deprecated(note = "use AppConfig::load() which also reads config.toml")]
pub fn load_from_env() -> Result<Self, AppError> {
    dotenvy::dotenv().ok();
    Self::load_from(|key| std::env::var(key).ok())
}
```

- [ ] **Step 3: Update existing unit tests to use SOBER_* env var names**

Update all existing tests in the `#[cfg(test)]` module to use `SOBER_DATABASE_URL` instead of
`DATABASE_URL`, `SOBER_SERVER_PORT` instead of `PORT`, `SOBER_ENV` stays `SOBER_ENV`
(unchanged), etc.

- [ ] **Step 4: Update integration tests to use SOBER_* env var names**

Update `backend/crates/sober-api/tests/http.rs` and `backend/crates/sober-api/tests/ws.rs` —
these use `AppConfig::load_from(env_from(&[("DATABASE_URL", ...)]))`; change to
`"SOBER_DATABASE_URL"`, `"SOBER_SERVER_PORT"`, etc. These tests WILL break if not updated now
because `load_from()` now uses the overlay which reads `SOBER_*` names.

- [ ] **Step 5: Run all tests including integration**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test --workspace -q 2>&1 | tail -20`

All tests must pass.

- [ ] **Step 6: Build full workspace**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo build -q 2>&1 | tail -20`

Fix any deprecation warnings or compilation issues in other crates.

- [ ] **Step 7: Commit**

```bash
git add backend/crates/sober-core/src/config.rs backend/crates/sober-api/tests/
git commit -m "feat(core): refactor load_from for SOBER_* vars, deprecate load_from_env"
```

---

### Task 8: Implement source tracking (LoadedConfig)

**Files:**
- Modify: `backend/crates/sober-core/src/config.rs`
- Modify: `backend/crates/sober-core/src/lib.rs`

- [ ] **Step 1: Write tests for source tracking**

```rust
#[test]
fn loaded_config_tracks_toml_source() {
    let toml_str = r#"
[database]
url = "postgres://test@localhost/test"
"#;
    let loaded = LoadedConfig::from_toml_str(toml_str, env_from(&[])).unwrap();
    assert_eq!(loaded.sources.database_url, Source::Toml);
    assert_eq!(loaded.sources.server_port, Source::Default);
}

#[test]
fn loaded_config_tracks_env_override() {
    let toml_str = r#"
[database]
url = "postgres://toml@localhost/test"
"#;
    let loaded = LoadedConfig::from_toml_str(
        toml_str,
        env_from(&[("SOBER_DATABASE_URL", "postgres://env@localhost/test")]),
    ).unwrap();
    assert_eq!(loaded.sources.database_url, Source::Env);
    assert_eq!(loaded.config.database.url, "postgres://env@localhost/test");
}
```

- [ ] **Step 2: Implement Source enum and ConfigSources struct**

```rust
/// Tracks where a config value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Compiled default.
    Default,
    /// Read from config.toml file.
    Toml,
    /// Overridden by environment variable.
    Env,
}

/// Parallel struct tracking the source of each config field.
///
/// Fields mirror `AppConfig` but store [`Source`] instead of values.
#[derive(Debug, Clone)]
pub struct ConfigSources {
    pub database_url: Source,
    pub database_max_connections: Source,
    pub qdrant_url: Source,
    // ... one field per config field, all default to Source::Default
    // (full list matches the env var mapping table)
}

/// Loaded configuration with per-field source tracking.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    /// The resolved configuration.
    pub config: AppConfig,
    /// Where each field's value came from.
    pub sources: ConfigSources,
    /// Path to the config file that was loaded, if any.
    pub config_path: Option<PathBuf>,
}
```

Implement `Default` for `ConfigSources` (all fields `Source::Default`).

- [ ] **Step 3: Add from_toml_str constructor and LoadedConfig::load()**

Add `LoadedConfig::from_toml_str(toml_str, get_var)` for testing. Add `LoadedConfig::load()`
as a separate method (does NOT change `AppConfig::load()` return type — binary callers keep
using `AppConfig::load() -> Result<AppConfig>`). `LoadedConfig::load()` calls
`AppConfig::load_resolved()` internally and builds source tracking alongside.

The overlay function sets source to `Source::Env` for each field it overwrites. TOML
deserialization sets non-default fields to `Source::Toml` by comparing against
`AppConfig::default()` after parsing.

- [ ] **Step 4: Re-export LoadedConfig from lib.rs**

Add `LoadedConfig` and `Source` to the re-exports in `sober-core/src/lib.rs`.

- [ ] **Step 5: Run tests**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test -p sober-core -q -- config::tests 2>&1 | tail -10`

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-core/src/config.rs backend/crates/sober-core/src/lib.rs
git commit -m "feat(core): add LoadedConfig with per-field source tracking"
```

---

### Task 9: Update binary callers — sober-api

**Files:**
- Modify: `backend/crates/sober-api/src/main.rs:28`

- [ ] **Step 1: Replace load_from_env() with load()**

Change `AppConfig::load_from_env()?` → `AppConfig::load()?` at line 28.

- [ ] **Step 2: Build and test**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo build -p sober-api -q && cargo test -p sober-api -q 2>&1 | tail -10`

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-api/src/main.rs
git commit -m "feat(api): use AppConfig::load() for layered config"
```

---

### Task 10: Update binary callers — sober-agent

**Files:**
- Modify: `backend/crates/sober-agent/src/main.rs:50,59-63,119-126,297-298`
- Modify: `backend/crates/sober-agent/src/agent.rs:379-380`

- [ ] **Step 1: Update main.rs to use AppConfig::load() and config.agent fields**

At line 50: `AppConfig::load_from_env()?` → `AppConfig::load()?`

Replace loose env var reads:
- Lines 59-63: `std::env::var("METRICS_PORT")...` → `config.agent.metrics_port`
- Lines 119-121: `std::env::var("WORKSPACE_ROOT")...` → `config.agent.workspace_root.to_string_lossy().into_owned()`
- Lines 122-126: `std::env::var("SANDBOX_PROFILE")...` → match on `config.agent.sandbox_profile.as_str()`
- Lines 297-298: `std::env::var("AGENT_SOCKET_PATH")...` → `config.agent.socket_path.to_string_lossy().into_owned()`

- [ ] **Step 2: Thread workspace_root into Agent struct**

In `agent.rs`, the `Agent` struct reads `WORKSPACE_ROOT` at line 379. Add a `workspace_root: PathBuf`
field to the `Agent` struct (or to the existing `AgentConfig` struct in `agent.rs`).

In `main.rs`, pass `config.agent.workspace_root.clone()` when constructing the `Agent`.

In `agent.rs:379`, replace `std::env::var("WORKSPACE_ROOT")...` with `self.workspace_root`
(or `self.config.workspace_root` if added to `AgentConfig`).

- [ ] **Step 3: Build and test**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo build -p sober-agent -q && cargo test -p sober-agent -q 2>&1 | tail -10`

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-agent/
git commit -m "feat(agent): use AppConfig for process config, remove loose env vars"
```

---

### Task 11: Update binary callers — sober-scheduler

**Files:**
- Modify: `backend/crates/sober-scheduler/src/main.rs:39,45-49,174-186`

- [ ] **Step 1: Update main.rs to use AppConfig::load() and config.scheduler fields**

At line 39: `AppConfig::load_from_env()?` → `AppConfig::load()?`

Replace loose env var reads:
- Lines 45-49: `std::env::var("METRICS_PORT")...` → `config.scheduler.metrics_port`
- Lines 174-175: `std::env::var("WORKSPACE_ROOT")...` → `config.scheduler.workspace_root.to_string_lossy().into_owned()`
- Lines 182-186: `std::env::var("SANDBOX_PROFILE")...` → match on `config.scheduler.sandbox_profile.as_str()`

- [ ] **Step 2: Build and test**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo build -p sober-scheduler -q && cargo test -p sober-scheduler -q 2>&1 | tail -10`

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-scheduler/
git commit -m "feat(scheduler): use AppConfig for process config, remove loose env vars"
```

---

### Task 12: Update binary callers — sober-web

**Files:**
- Modify: `backend/crates/sober-web/src/main.rs:46-58`

- [ ] **Step 1: Add AppConfig::load_unvalidated() and use config.web fields**

`sober-web` doesn't need a database URL or other validated fields — it only uses `[web]`
section. Use `load_unvalidated()` to avoid requiring DATABASE_URL.

Replace the raw `std::env::var()` calls at lines 46-58 with:

```rust
let config = AppConfig::load_unvalidated()?;
let host = config.web.host;
let port = config.web.port;
let api_upstream = config.web.api_upstream_url;
let static_dir = config.web.static_dir;
```

Keep the early `SOBER_ENV` read for telemetry initialization (before config load).

Add `use sober_core::config::AppConfig;` if not already imported.

- [ ] **Step 2: Build and test**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo build -p sober-web -q 2>&1 | tail -10`

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-web/
git commit -m "feat(web): use AppConfig for process config, remove loose env vars"
```

---

### Task 13: Update CLI — config commands + connect_db

**Files:**
- Modify: `backend/crates/sober-cli/src/cli.rs:106-113`
- Modify: `backend/crates/sober-cli/src/commands/config.rs`
- Modify: `backend/crates/sober-cli/src/sober.rs:75-89`

- [ ] **Step 1: Add ConfigCommand::Generate variant**

In `cli.rs`, add to the `ConfigCommand` enum:

```rust
/// Generate a default configuration file to stdout.
Generate,
```

- [ ] **Step 2: Update connect_db() to use AppConfig**

In `sober.rs`, change `connect_db()` to load config via `AppConfig::load()` and use
`config.database.url` instead of raw `std::env::var("DATABASE_URL")`.

- [ ] **Step 3: Update config show command**

In `commands/config.rs`, update `show()`:
- Use `LoadedConfig::load()` instead of `AppConfig::load_from_env()`
- Accept a `source: bool` parameter (from `--source` CLI flag)
- Add missing `embedding_dim` display
- Add `agent` and `web` sections to output
- When `source` is true, append `(toml)`, `(env)`, or `(default)` suffix to each line
- For `None` optional sections (e.g., `[acp]` not configured), display `(not configured)`
  with no source annotation
- Add missing fields to output: `embedding_dim`, `rate_limit_max_requests`,
  `rate_limit_window_secs`, `[crypto]` section, new `[agent]` and `[web]` sections

Add `--source` flag: in `cli.rs`, add to the `Show` variant:

```rust
/// Display the resolved configuration (secrets redacted).
Show {
    /// Show where each value came from (default/toml/env).
    #[arg(long)]
    source: bool,
},
```

- [ ] **Step 4: Update config validate command**

In `commands/config.rs`, update `validate()`:
- Use `AppConfig::load()` (with validation) instead of `load_from_env()`
- On success, print source summary (e.g., "Loaded from /etc/sober/config.toml + N env overrides")

Note: `config show` and `config generate` use `load_unvalidated()` / `LoadedConfig::load()`
(which also skips validation) since displaying config shouldn't require all fields to be valid.
`config validate` is the one command that explicitly validates.

- [ ] **Step 5: Implement config generate command**

In `commands/config.rs`, add `generate()`:

```rust
/// Generate a complete default configuration file to stdout.
pub fn generate() -> Result<()> {
    // Hand-written TOML output with comments.
    // Each section has field comments above, secrets commented out.
    print!(r#"# Sõber Configuration File
# Generated by: sober config generate
# Docs: https://github.com/harrisiirak/s-ber

# Runtime environment: "production" or "development"
environment = "development"

[database]
# PostgreSQL connection URL (required)
url = ""
# Maximum connection pool size
max_connections = 10

..."#);
    Ok(())
}
```

The full output should match the TOML structure in the design spec, with comments above each
field, secrets commented out (e.g., `# api_key = "sk-..."`), and optional sections like `[acp]`
fully commented out.

- [ ] **Step 6: Wire Generate in sober.rs**

In the match on `ConfigCommand`, add:

```rust
ConfigCommand::Generate => commands::config::generate()?,
```

- [ ] **Step 7: Build and test**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo build -p sober-cli -q && cargo test -p sober-cli -q 2>&1 | tail -10`

- [ ] **Step 8: Commit**

```bash
git add backend/crates/sober-cli/
git commit -m "feat(cli): enhance config show/validate, add config generate"
```

---

### Task 14: Update .env.example

**Files:**
- Rewrite: `.env.example`

- [ ] **Step 1: Rewrite .env.example with SOBER_* prefix**

Replace the entire file with all env vars using the `SOBER_*` naming convention. Include:
- All vars from the design spec env var mapping table
- New vars: `SOBER_LLM_EMBEDDING_DIM`, `SOBER_AGENT_*`, `SOBER_SCHEDULER_METRICS_PORT`,
  `SOBER_SCHEDULER_WORKSPACE_ROOT`, `SOBER_SCHEDULER_SANDBOX_PROFILE`, `SOBER_WEB_*`
- Fix `SCHEDULER_ADMIN_SOCKET_PATH` → `SOBER_SCHEDULER_SOCKET_PATH`
- Keep `RUST_LOG`, `OTEL_*`, `PUBLIC_API_URL` unchanged (not part of `AppConfig`)
- Group by section with comment headers matching TOML sections

- [ ] **Step 2: Commit**

```bash
git add .env.example
git commit -m "feat: migrate .env.example to SOBER_* prefix"
```

---

### Task 15: Update docker-compose.yml

**Files:**
- Modify: `docker-compose.yml`

- [ ] **Step 1: Update all env var references to SOBER_* prefix**

Rename every env var in the file:
- All `x-*-env` YAML anchors at the top
- `migrate` service: `DATABASE_URL` → `SOBER_DATABASE_URL`
- `sober-agent` service: `AGENT_SOCKET_PATH` → `SOBER_AGENT_SOCKET_PATH`, `SANDBOX_PROFILE` →
  `SOBER_AGENT_SANDBOX_PROFILE`, add `SOBER_AGENT_METRICS_PORT`, `SOBER_AGENT_WORKSPACE_ROOT`
- `sober-agent` service: also rename `ADMIN_SOCKET_PATH` → `SOBER_ADMIN_SOCKET_PATH`
  (line 142 — this is separate from the agent's gRPC socket)
- `sober-api` service: `HOST` → `SOBER_SERVER_HOST`, `PORT` → `SOBER_SERVER_PORT`,
  `ADMIN_SOCKET_PATH` → `SOBER_ADMIN_SOCKET_PATH`, `SESSION_SECRET` → `SOBER_AUTH_SESSION_SECRET`,
  `SESSION_TTL_SECONDS` → `SOBER_AUTH_SESSION_TTL_SECONDS`, `SEARXNG_URL` → `SOBER_SEARXNG_URL`
- `sober-scheduler` service: all `SCHEDULER_*` → `SOBER_SCHEDULER_*`,
  fix `SCHEDULER_ADMIN_SOCKET_PATH` → `SOBER_SCHEDULER_SOCKET_PATH` with value
  `/run/sober/scheduler.sock`, add metrics/workspace/sandbox vars
- `sober-web` service: `HOST` → `SOBER_WEB_HOST`, `PORT` → `SOBER_WEB_PORT`,
  `API_UPSTREAM_URL` → `SOBER_WEB_API_UPSTREAM_URL`, `STATIC_DIR` → `SOBER_WEB_STATIC_DIR`

- [ ] **Step 2: Build Docker to verify**

Run: `docker compose config -q 2>&1 | tail -5` (validates YAML syntax).

- [ ] **Step 3: Commit**

```bash
git add docker-compose.yml
git commit -m "feat: migrate docker-compose.yml to SOBER_* prefix"
```

---

### Task 16: Update config.toml.example and install.sh

**Files:**
- Rewrite: `infra/config/config.toml.example`
- Modify: `scripts/install.sh:238-283`

- [ ] **Step 1: Rewrite config.toml.example**

Replace the entire file with the full TOML structure from the design spec, with comment headers
for each section. Drop the old `[storage]` and `[logging]` sections.

- [ ] **Step 2: Update install.sh write_default_config()**

Replace the `write_default_config()` function (lines 238-256) to write a complete `config.toml`
covering all sections with sensible defaults.

- [ ] **Step 3: Update install.sh collect_config()**

Replace `collect_config()` (lines 258-283) to:
- Prompt for values as before (DATABASE_URL, LLM_BASE_URL, LLM_API_KEY, LLM_MODEL)
- Write values directly into `config.toml` using `sed` to replace placeholder values
- Stop creating `.env` for new installs
- Keep checking for existing `.env` on upgrades

- [ ] **Step 4: Commit**

```bash
git add infra/config/config.toml.example scripts/install.sh
git commit -m "feat: update config.toml.example and install.sh for full TOML config"
```

---

### Task 17: Integration verification

Integration tests were already updated to SOBER_* names in Task 7. This task is a final
verification pass.

- [ ] **Step 1: Run full workspace tests**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo test --workspace -q 2>&1 | tail -20`

All tests must pass.

- [ ] **Step 2: Run clippy on full workspace**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo clippy --workspace -q -- -D warnings 2>&1 | tail -20`

Fix any warnings.

- [ ] **Step 3: Run fmt check**

Run: `cd /home/harri/Projects/Repos/sober/backend && cargo fmt --check -q 2>&1 | tail -5`

- [ ] **Step 4: Commit any fixes**

```bash
git add -A
git commit -m "fix: integration verification fixes"
```

---

### Task 18: Move plan folder and final commit

- [ ] **Step 1: Move plan folder from active to done**

```bash
git mv docs/plans/active/036-config-toml docs/plans/done/036-config-toml
```

- [ ] **Step 2: Final commit**

```bash
git add docs/plans/
git commit -m "docs: move plan 036 to done"
```
