//! Application configuration loaded from environment variables.
//!
//! [`AppConfig`] is a monolithic config struct used by all three binaries
//! (`sober-api`, `sober-agent`, `sober-scheduler`). Each section handles its
//! own validation — binaries that don't need a section won't fail if those
//! env vars are absent.

use std::path::PathBuf;

use crate::error::AppError;

/// Top-level application configuration.
///
/// Loaded from environment variables at startup via [`load_from_env`](AppConfig::load_from_env).
/// Unused sections are harmless — each section applies its own defaults.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// PostgreSQL connection settings.
    pub database: DatabaseConfig,
    /// Qdrant vector database settings.
    pub qdrant: QdrantConfig,
    /// LLM provider settings.
    pub llm: LlmConfig,
    /// HTTP server settings.
    pub server: ServerConfig,
    /// Authentication/session settings.
    pub auth: AuthConfig,
    /// SearXNG search engine settings.
    pub searxng: SearxngConfig,
    /// Admin Unix socket settings.
    pub admin: AdminConfig,
    /// Scheduler settings.
    pub scheduler: SchedulerConfig,
    /// MCP server settings.
    pub mcp: McpConfig,
    /// Memory system settings.
    pub memory: MemoryConfig,
    /// ACP agent settings (optional — only needed when using ACP transport).
    pub acp: Option<AcpAgentConfig>,
    /// Envelope encryption settings (secrets management).
    pub crypto: CryptoConfig,
    /// Runtime environment (development or production).
    pub environment: Environment,
    /// Agent process settings (socket, metrics, workspace).
    #[serde(rename = "agent")]
    pub agent: AgentProcessConfig,
    /// Web reverse-proxy settings.
    pub web: WebConfig,
}

/// Runtime environment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    /// Local development with pretty logging.
    #[default]
    Development,
    /// Production with JSON logging.
    Production,
}

/// PostgreSQL connection settings.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    /// PostgreSQL connection URL.
    pub url: String,
    /// Maximum number of connections in the pool.
    pub max_connections: u32,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            max_connections: 10,
        }
    }
}

/// Qdrant vector database settings.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct QdrantConfig {
    /// Qdrant gRPC/HTTP URL.
    pub url: String,
    /// Optional API key for authentication.
    pub api_key: Option<String>,
}

impl Default for QdrantConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:6334".to_owned(),
            api_key: None,
        }
    }
}

/// LLM provider settings.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// Base URL for the OpenAI-compatible API endpoint.
    pub base_url: String,
    /// API key for authentication (optional for local models).
    pub api_key: Option<String>,
    /// Default model identifier.
    pub model: String,
    /// Maximum tokens per completion request.
    pub max_tokens: u32,
    /// Model used for embedding generation.
    pub embedding_model: String,
    /// Dimensionality of the embedding vectors produced by the embedding model.
    pub embedding_dim: u64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: "https://openrouter.ai/api/v1".to_owned(),
            api_key: None,
            model: "anthropic/claude-sonnet-4".to_owned(),
            max_tokens: 4096,
            embedding_model: "text-embedding-3-small".to_owned(),
            embedding_dim: 1536,
        }
    }
}

/// HTTP server settings.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Bind address (e.g. `0.0.0.0`).
    pub host: String,
    /// Bind port.
    pub port: u16,
    /// Maximum requests allowed per rate-limit window.
    pub rate_limit_max_requests: u32,
    /// Rate-limit window duration in seconds.
    pub rate_limit_window_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_owned(),
            port: 3000,
            rate_limit_max_requests: 1200,
            rate_limit_window_secs: 60,
        }
    }
}

/// Authentication and session settings.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    /// Secret used for session token signing (required in production).
    pub session_secret: Option<String>,
    /// Session time-to-live in seconds.
    pub session_ttl_seconds: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            session_secret: None,
            session_ttl_seconds: 2_592_000,
        }
    }
}

/// SearXNG search engine settings.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct SearxngConfig {
    /// SearXNG instance URL.
    pub url: String,
}

impl Default for SearxngConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:8080".to_owned(),
        }
    }
}

/// Admin Unix socket settings.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct AdminConfig {
    /// Path to the admin Unix domain socket.
    pub socket_path: PathBuf,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            socket_path: PathBuf::from("/run/sober/admin.sock"),
        }
    }
}

/// Scheduler settings.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct SchedulerConfig {
    /// Tick interval in seconds.
    pub tick_interval_secs: u64,
    /// Path to the agent gRPC socket.
    pub agent_socket_path: PathBuf,
    /// Path to the scheduler's own gRPC socket.
    pub socket_path: PathBuf,
    /// Maximum number of concurrent jobs.
    pub max_concurrent_jobs: u32,
    /// Prometheus metrics port.
    pub metrics_port: u16,
    /// Root directory for workspaces.
    pub workspace_root: PathBuf,
    /// Sandbox profile name.
    pub sandbox_profile: String,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tick_interval_secs: 1,
            agent_socket_path: PathBuf::from("/run/sober/agent.sock"),
            socket_path: PathBuf::from("/run/sober/scheduler.sock"),
            max_concurrent_jobs: 10,
            metrics_port: 9101,
            workspace_root: PathBuf::from("/var/lib/sober/workspaces"),
            sandbox_profile: "standard".to_owned(),
        }
    }
}

/// MCP server connection settings.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct McpConfig {
    /// Request timeout in seconds.
    pub request_timeout_secs: u64,
    /// Maximum consecutive failures before marking a server unhealthy.
    pub max_consecutive_failures: u32,
    /// Idle timeout in seconds before disconnecting.
    pub idle_timeout_secs: u64,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            request_timeout_secs: 30,
            max_consecutive_failures: 3,
            idle_timeout_secs: 300,
        }
    }
}

/// ACP (Agent Client Protocol) agent settings.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct AcpAgentConfig {
    /// Display name for the agent.
    pub name: String,
    /// Binary to spawn (e.g. `"claude"`, `"kimi"`).
    pub command: String,
    /// CLI arguments (e.g. `["acp"]`).
    pub args: Vec<String>,
}

impl Default for AcpAgentConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            command: String::new(),
            args: vec!["acp".to_owned()],
        }
    }
}

/// Memory system tuning parameters.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    /// Half-life for importance score decay, in days.
    pub decay_half_life_days: u32,
    /// Boost factor applied to recently retrieved memories.
    pub retrieval_boost: f64,
    /// Importance threshold below which memories are pruned.
    pub prune_threshold: f64,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            decay_half_life_days: 30,
            retrieval_boost: 0.2,
            prune_threshold: 0.1,
        }
    }
}

/// Envelope encryption settings for secrets management.
#[derive(Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct CryptoConfig {
    /// Hex-encoded 256-bit master encryption key for envelope encryption.
    ///
    /// Required when secrets management is enabled. Generate with:
    /// `openssl rand -hex 32`
    pub master_encryption_key: Option<String>,
}

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

impl std::fmt::Debug for CryptoConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CryptoConfig")
            .field(
                "master_encryption_key",
                &self.master_encryption_key.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

impl AppConfig {
    /// Discovers the config file path using the search order:
    /// 1. `SOBER_CONFIG` env var (explicit path)
    /// 2. `/etc/sober/config.toml` (production)
    /// 3. `./config.toml` (development)
    ///
    /// Returns `None` if no config file is found. Accepts closures for testability.
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

    /// Internal: discover TOML + overlay env vars, no validation.
    fn load_resolved() -> Result<Self, AppError> {
        dotenvy::dotenv().ok();

        let config_path = Self::discover_config_path(|key| std::env::var(key).ok(), |p| p.exists());

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
    /// Discovery order: `SOBER_CONFIG` env var, `/etc/sober/config.toml`, `./config.toml`.
    /// Falls back to defaults if no file is found. Environment variables always win.
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

    /// Overlays `SOBER_*` environment variables onto the config, overwriting any
    /// TOML or default values. Each set env var wins over the file/default value.
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
        if let Some(v) = get_var("SOBER_DATABASE_URL") {
            self.database.url = v;
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_DATABASE_MAX_CONNECTIONS") {
            self.database.max_connections = v;
        }

        // qdrant
        if let Some(v) = get_var("SOBER_QDRANT_URL") {
            self.qdrant.url = v;
        }
        if let Some(v) = get_var("SOBER_QDRANT_API_KEY") {
            self.qdrant.api_key = Some(v);
        }

        // llm
        if let Some(v) = get_var("SOBER_LLM_BASE_URL") {
            self.llm.base_url = v;
        }
        if let Some(v) = get_var("SOBER_LLM_API_KEY") {
            self.llm.api_key = Some(v);
        }
        if let Some(v) = get_var("SOBER_LLM_MODEL") {
            self.llm.model = v;
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_LLM_MAX_TOKENS") {
            self.llm.max_tokens = v;
        }
        if let Some(v) = get_var("SOBER_LLM_EMBEDDING_MODEL") {
            self.llm.embedding_model = v;
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_LLM_EMBEDDING_DIM") {
            self.llm.embedding_dim = v;
        }

        // server
        if let Some(v) = get_var("SOBER_SERVER_HOST") {
            self.server.host = v;
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_SERVER_PORT") {
            self.server.port = v;
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_SERVER_RATE_LIMIT_MAX_REQUESTS") {
            self.server.rate_limit_max_requests = v;
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_SERVER_RATE_LIMIT_WINDOW_SECS") {
            self.server.rate_limit_window_secs = v;
        }

        // auth
        if let Some(v) = get_var("SOBER_AUTH_SESSION_SECRET") {
            self.auth.session_secret = Some(v);
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_AUTH_SESSION_TTL_SECONDS") {
            self.auth.session_ttl_seconds = v;
        }

        // searxng
        if let Some(v) = get_var("SOBER_SEARXNG_URL") {
            self.searxng.url = v;
        }

        // admin
        if let Some(v) = get_var("SOBER_ADMIN_SOCKET_PATH") {
            self.admin.socket_path = PathBuf::from(v);
        }

        // agent (process config)
        if let Some(v) = get_var("SOBER_AGENT_SOCKET_PATH") {
            self.agent.socket_path = PathBuf::from(v);
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_AGENT_METRICS_PORT") {
            self.agent.metrics_port = v;
        }
        if let Some(v) = get_var("SOBER_AGENT_WORKSPACE_ROOT") {
            self.agent.workspace_root = PathBuf::from(v);
        }
        if let Some(v) = get_var("SOBER_AGENT_SANDBOX_PROFILE") {
            self.agent.sandbox_profile = v;
        }

        // scheduler
        if let Ok(Some(v)) = env.parse_opt("SOBER_SCHEDULER_TICK_INTERVAL_SECS") {
            self.scheduler.tick_interval_secs = v;
        }
        if let Some(v) = get_var("SOBER_SCHEDULER_AGENT_SOCKET_PATH") {
            self.scheduler.agent_socket_path = PathBuf::from(v);
        }
        if let Some(v) = get_var("SOBER_SCHEDULER_SOCKET_PATH") {
            self.scheduler.socket_path = PathBuf::from(v);
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_SCHEDULER_MAX_CONCURRENT_JOBS") {
            self.scheduler.max_concurrent_jobs = v;
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_SCHEDULER_METRICS_PORT") {
            self.scheduler.metrics_port = v;
        }
        if let Some(v) = get_var("SOBER_SCHEDULER_WORKSPACE_ROOT") {
            self.scheduler.workspace_root = PathBuf::from(v);
        }
        if let Some(v) = get_var("SOBER_SCHEDULER_SANDBOX_PROFILE") {
            self.scheduler.sandbox_profile = v;
        }

        // web
        if let Some(v) = get_var("SOBER_WEB_HOST") {
            self.web.host = v;
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_WEB_PORT") {
            self.web.port = v;
        }
        if let Some(v) = get_var("SOBER_WEB_API_UPSTREAM_URL") {
            self.web.api_upstream_url = v;
        }
        if let Some(v) = get_var("SOBER_WEB_STATIC_DIR") {
            self.web.static_dir = Some(PathBuf::from(v));
        }

        // mcp
        if let Ok(Some(v)) = env.parse_opt("SOBER_MCP_REQUEST_TIMEOUT_SECS") {
            self.mcp.request_timeout_secs = v;
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_MCP_MAX_CONSECUTIVE_FAILURES") {
            self.mcp.max_consecutive_failures = v;
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_MCP_IDLE_TIMEOUT_SECS") {
            self.mcp.idle_timeout_secs = v;
        }

        // memory
        if let Ok(Some(v)) = env.parse_opt("SOBER_MEMORY_DECAY_HALF_LIFE_DAYS") {
            self.memory.decay_half_life_days = v;
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_MEMORY_RETRIEVAL_BOOST") {
            self.memory.retrieval_boost = v;
        }
        if let Ok(Some(v)) = env.parse_opt("SOBER_MEMORY_PRUNE_THRESHOLD") {
            self.memory.prune_threshold = v;
        }

        // crypto
        if let Some(v) = get_var("SOBER_CRYPTO_MASTER_ENCRYPTION_KEY") {
            self.crypto.master_encryption_key = Some(v);
        }

        // acp (optional section — any SOBER_ACP_* env var triggers Some)
        let acp_command = get_var("SOBER_ACP_COMMAND");
        let acp_name = get_var("SOBER_ACP_NAME");
        let acp_args = get_var("SOBER_ACP_ARGS");
        if acp_command.is_some() || acp_name.is_some() || acp_args.is_some() {
            let acp = self.acp.get_or_insert_with(AcpAgentConfig::default);
            if let Some(v) = acp_command {
                acp.command = v;
            }
            if let Some(v) = acp_name {
                acp.name = v;
            }
            if let Some(v) = acp_args {
                acp.args = v.split_whitespace().map(String::from).collect();
            }
        }
    }

    /// Validates the loaded configuration.
    ///
    /// Called after TOML loading + env overlay. Checks required fields and format constraints.
    ///
    /// This is a no-op placeholder — the full validation is added in a subsequent commit.
    pub fn validate(&self) -> Result<(), AppError> {
        // Will be implemented in Task 6.
        Ok(())
    }

    /// Loads configuration from environment variables.
    ///
    /// Calls `dotenvy::dotenv().ok()` first to load `.env` if present.
    /// Returns [`AppError::Validation`] for missing or malformed values.
    pub fn load_from_env() -> Result<Self, AppError> {
        dotenvy::dotenv().ok();
        Self::load_from(|key| std::env::var(key).ok())
    }

    /// Loads configuration from an arbitrary key-value source.
    ///
    /// The `get_var` closure is called for each configuration key.
    /// Returns `None` when a key is absent.
    pub fn load_from(get_var: impl Fn(&str) -> Option<String>) -> Result<Self, AppError> {
        let env = EnvSource(get_var);

        let environment = match env.opt("SOBER_ENV").as_deref() {
            Some("production") => Environment::Production,
            _ => Environment::Development,
        };

        Ok(Self {
            database: DatabaseConfig {
                url: env.required("DATABASE_URL")?,
                max_connections: env.parse("DATABASE_MAX_CONNECTIONS", 10)?,
            },
            qdrant: QdrantConfig {
                url: env.or("QDRANT_URL", "http://localhost:6334"),
                api_key: env.opt("QDRANT_API_KEY"),
            },
            llm: LlmConfig {
                base_url: env.or("LLM_BASE_URL", "https://openrouter.ai/api/v1"),
                api_key: env.opt("LLM_API_KEY"),
                model: env.or("LLM_MODEL", "anthropic/claude-sonnet-4"),
                max_tokens: env.parse("LLM_MAX_TOKENS", 4096)?,
                embedding_model: env.or("EMBEDDING_MODEL", "text-embedding-3-small"),
                embedding_dim: env.parse("EMBEDDING_DIM", 1536)?,
            },
            server: ServerConfig {
                host: env.or("HOST", "0.0.0.0"),
                port: env.parse("PORT", 3000)?,
                rate_limit_max_requests: env.parse("RATE_LIMIT_MAX_REQUESTS", 1200)?,
                rate_limit_window_secs: env.parse("RATE_LIMIT_WINDOW_SECS", 60)?,
            },
            auth: AuthConfig {
                session_secret: env.opt("SESSION_SECRET"),
                session_ttl_seconds: env.parse("SESSION_TTL_SECONDS", 2_592_000)?,
            },
            searxng: SearxngConfig {
                url: env.or("SEARXNG_URL", "http://localhost:8080"),
            },
            admin: AdminConfig {
                socket_path: PathBuf::from(env.or("ADMIN_SOCKET_PATH", "/run/sober/admin.sock")),
            },
            scheduler: SchedulerConfig {
                tick_interval_secs: env.parse("SCHEDULER_TICK_INTERVAL_SECS", 1)?,
                agent_socket_path: PathBuf::from(
                    env.or("SCHEDULER_AGENT_SOCKET_PATH", "/run/sober/agent.sock"),
                ),
                socket_path: PathBuf::from(
                    env.or("SCHEDULER_SOCKET_PATH", "/run/sober/scheduler.sock"),
                ),
                max_concurrent_jobs: env.parse("SCHEDULER_MAX_CONCURRENT_JOBS", 10)?,
                metrics_port: 9101,
                workspace_root: PathBuf::from("/var/lib/sober/workspaces"),
                sandbox_profile: "standard".to_owned(),
            },
            mcp: McpConfig {
                request_timeout_secs: env.parse("MCP_REQUEST_TIMEOUT_SECS", 30)?,
                max_consecutive_failures: env.parse("MCP_MAX_CONSECUTIVE_FAILURES", 3)?,
                idle_timeout_secs: env.parse("MCP_IDLE_TIMEOUT_SECS", 300)?,
            },
            memory: MemoryConfig {
                decay_half_life_days: env.parse("MEMORY_DECAY_HALF_LIFE_DAYS", 30)?,
                retrieval_boost: env.parse("MEMORY_RETRIEVAL_BOOST", 0.2)?,
                prune_threshold: env.parse("MEMORY_PRUNE_THRESHOLD", 0.1)?,
            },
            acp: env.opt("ACP_AGENT_COMMAND").map(|command| {
                let args_str = env.or("ACP_AGENT_ARGS", "acp");
                AcpAgentConfig {
                    name: env.or("ACP_AGENT_NAME", &command),
                    command,
                    args: args_str.split_whitespace().map(String::from).collect(),
                }
            }),
            crypto: CryptoConfig {
                master_encryption_key: env.opt("MASTER_ENCRYPTION_KEY"),
            },
            environment,
            agent: AgentProcessConfig::default(),
            web: WebConfig::default(),
        })
    }
}

/// Wraps a key-lookup closure with helper methods for common patterns.
struct EnvSource<F: Fn(&str) -> Option<String>>(F);

impl<F: Fn(&str) -> Option<String>> EnvSource<F> {
    /// Reads a required key, returning an error if absent.
    fn required(&self, key: &str) -> Result<String, AppError> {
        (self.0)(key).ok_or_else(|| {
            AppError::Validation(format!("missing required environment variable: {key}"))
        })
    }

    /// Reads a key with a default fallback.
    fn or(&self, key: &str, default: &str) -> String {
        (self.0)(key).unwrap_or_else(|| default.to_owned())
    }

    /// Reads an optional key.
    fn opt(&self, key: &str) -> Option<String> {
        (self.0)(key)
    }

    /// Reads and parses a key, falling back to a default.
    fn parse<T>(&self, key: &str, default: T) -> Result<T, AppError>
    where
        T: std::str::FromStr + std::fmt::Display,
        T::Err: std::fmt::Display,
    {
        match (self.0)(key) {
            Some(val) => val.parse::<T>().map_err(|e| {
                AppError::Validation(format!("invalid value for {key}: {e} (got: {val})"))
            }),
            None => Ok(default),
        }
    }

    /// Parses an optional env var. Returns `Ok(None)` if absent, `Ok(Some(v))` if valid.
    fn parse_opt<T>(&self, key: &str) -> Result<Option<T>, AppError>
    where
        T: std::str::FromStr,
        T::Err: std::fmt::Display,
    {
        match (self.0)(key) {
            Some(val) => val.parse::<T>().map(Some).map_err(|e| {
                AppError::Validation(format!("invalid value for {key}: {e} (got: {val})"))
            }),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::collections::HashMap;

    /// Creates a lookup closure from a set of key-value pairs.
    fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn load_with_database_url_set() {
        let config = AppConfig::load_from(env_from(&[(
            "DATABASE_URL",
            "postgres://test:test@localhost/test",
        )]))
        .unwrap();
        assert_eq!(config.database.url, "postgres://test:test@localhost/test");
        assert_eq!(config.database.max_connections, 10);
        assert_eq!(config.server.port, 3000);
        assert_eq!(config.environment, Environment::Development);
    }

    #[test]
    fn missing_database_url_returns_validation_error() {
        let result = AppConfig::load_from(env_from(&[]));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::Validation(msg) if msg.contains("DATABASE_URL")));
    }

    #[test]
    fn production_environment() {
        let config = AppConfig::load_from(env_from(&[
            ("DATABASE_URL", "postgres://test:test@localhost/test"),
            ("SOBER_ENV", "production"),
        ]))
        .unwrap();
        assert_eq!(config.environment, Environment::Production);
    }

    #[test]
    fn invalid_port_returns_validation_error() {
        let result = AppConfig::load_from(env_from(&[
            ("DATABASE_URL", "postgres://test:test@localhost/test"),
            ("PORT", "not_a_number"),
        ]));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::Validation(msg) if msg.contains("PORT")));
    }

    #[test]
    fn defaults_applied_correctly() {
        let config = AppConfig::load_from(env_from(&[(
            "DATABASE_URL",
            "postgres://test:test@localhost/test",
        )]))
        .unwrap();
        assert_eq!(config.scheduler.tick_interval_secs, 1);
        assert_eq!(config.scheduler.max_concurrent_jobs, 10);
        assert_eq!(config.mcp.request_timeout_secs, 30);
        assert_eq!(config.mcp.max_consecutive_failures, 3);
        assert_eq!(config.memory.decay_half_life_days, 30);
        assert_eq!(config.memory.prune_threshold, 0.1);
    }

    #[test]
    fn optional_fields_are_none_by_default() {
        let config = AppConfig::load_from(env_from(&[(
            "DATABASE_URL",
            "postgres://test:test@localhost/test",
        )]))
        .unwrap();
        assert!(config.llm.api_key.is_none());
        assert!(config.auth.session_secret.is_none());
        assert!(config.qdrant.api_key.is_none());
    }

    #[test]
    fn optional_fields_populated_when_set() {
        let config = AppConfig::load_from(env_from(&[
            ("DATABASE_URL", "postgres://test:test@localhost/test"),
            ("LLM_API_KEY", "sk-test"),
            ("SESSION_SECRET", "my-secret"),
        ]))
        .unwrap();
        assert_eq!(config.llm.api_key.as_deref(), Some("sk-test"));
        assert_eq!(config.auth.session_secret.as_deref(), Some("my-secret"));
    }

    // ── Task 1: Environment Deserialize + Default ───────────────────────────

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

    // ── Task 2: All config structs Deserialize + Default ─────────────────────

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

    // ── Task 3: New config structs ───────────────────────────────────────────

    #[test]
    fn agent_process_config_defaults() {
        let cfg = AgentProcessConfig::default();
        assert_eq!(cfg.socket_path, PathBuf::from("/run/sober/agent.sock"));
        assert_eq!(cfg.metrics_port, 9100);
        assert_eq!(
            cfg.workspace_root,
            PathBuf::from("/var/lib/sober/workspaces")
        );
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
        assert_eq!(
            cfg.workspace_root,
            PathBuf::from("/var/lib/sober/workspaces")
        );
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
        assert_eq!(config.server.port, 3000);
        assert!(config.acp.is_none());
    }

    // ── Task 4: Config file discovery + TOML loading ────────────────────────

    #[test]
    fn discover_config_returns_none_when_no_file() {
        let path = AppConfig::discover_config_path(
            |_| None,
            |p| p == std::path::Path::new("/nonexistent"),
        );
        assert!(path.is_none());
    }

    #[test]
    fn discover_config_prefers_sober_config_env() {
        let path = AppConfig::discover_config_path(
            |key| {
                if key == "SOBER_CONFIG" {
                    Some("/custom/config.toml".to_owned())
                } else {
                    None
                }
            },
            |_| true,
        );
        assert_eq!(path, Some(PathBuf::from("/custom/config.toml")));
    }

    #[test]
    fn discover_config_falls_through_to_etc() {
        let path = AppConfig::discover_config_path(
            |_| None,
            |p| p == std::path::Path::new("/etc/sober/config.toml"),
        );
        assert_eq!(path, Some(PathBuf::from("/etc/sober/config.toml")));
    }

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
        assert_eq!(config.llm.model, "anthropic/claude-sonnet-4");
    }

    // ── Task 5: Env var overlay ─────────────────────────────────────────────

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
}
