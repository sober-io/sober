//! Application configuration loaded from environment variables.
//!
//! [`AppConfig`] is a monolithic config struct used by all three binaries
//! (`sober-api`, `sober-agent`, `sober-scheduler`). Each section handles its
//! own validation — binaries that don't need a section won't fail if those
//! env vars are absent.

use std::path::PathBuf;

use crate::error::AppError;

// ── Default constants ────────────────────────────────────────────────────────

/// Default bind address for servers.
pub const DEFAULT_HOST: &str = "0.0.0.0";
/// Default API server port.
pub const DEFAULT_API_PORT: u16 = 3000;
/// Default web reverse-proxy port.
pub const DEFAULT_WEB_PORT: u16 = 8080;
/// Default database connection pool size.
pub const DEFAULT_DB_MAX_CONNECTIONS: u32 = 10;
/// Default Qdrant URL.
pub const DEFAULT_QDRANT_URL: &str = "http://localhost:6334";
/// Default OpenAI-compatible LLM API base URL.
pub const DEFAULT_LLM_BASE_URL: &str = "https://openrouter.ai/api/v1";
/// Default LLM model identifier.
pub const DEFAULT_LLM_MODEL: &str = "anthropic/claude-sonnet-4";
/// Default max tokens per LLM completion.
pub const DEFAULT_LLM_MAX_TOKENS: u32 = 4096;
/// Default embedding model.
pub const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";
/// Default embedding vector dimensionality.
pub const DEFAULT_EMBEDDING_DIM: u64 = 1536;
/// Default rate limit: max requests per window.
pub const DEFAULT_RATE_LIMIT_MAX_REQUESTS: u32 = 1200;
/// Default rate limit window in seconds.
pub const DEFAULT_RATE_LIMIT_WINDOW_SECS: u64 = 60;
/// Default session TTL: 30 days in seconds.
pub const DEFAULT_SESSION_TTL_SECONDS: u64 = 2_592_000;
/// Default SearXNG URL.
pub const DEFAULT_SEARXNG_URL: &str = "http://localhost:8080";
/// Default admin socket path.
pub const DEFAULT_ADMIN_SOCKET_PATH: &str = "/run/sober/admin.sock";
/// Default agent gRPC socket path.
pub const DEFAULT_AGENT_SOCKET_PATH: &str = "/run/sober/agent.sock";
/// Default agent metrics port.
pub const DEFAULT_AGENT_METRICS_PORT: u16 = 9100;
/// Default scheduler metrics port.
pub const DEFAULT_SCHEDULER_METRICS_PORT: u16 = 9101;
/// Default scheduler tick interval in seconds.
pub const DEFAULT_SCHEDULER_TICK_INTERVAL_SECS: u64 = 1;
/// Default scheduler socket path.
pub const DEFAULT_SCHEDULER_SOCKET_PATH: &str = "/run/sober/scheduler.sock";
/// Default maximum concurrent scheduler jobs.
pub const DEFAULT_SCHEDULER_MAX_CONCURRENT_JOBS: u32 = 10;
/// Default workspace root directory.
pub const DEFAULT_WORKSPACE_ROOT: &str = "/var/lib/sober/workspaces";
/// Default sandbox profile.
pub const DEFAULT_SANDBOX_PROFILE: &str = "standard";
/// Default upstream API URL for the web reverse-proxy.
pub const DEFAULT_WEB_API_UPSTREAM_URL: &str = "http://localhost:3000";
/// Default MCP request timeout in seconds.
pub const DEFAULT_MCP_REQUEST_TIMEOUT_SECS: u64 = 30;
/// Default MCP max consecutive failures.
pub const DEFAULT_MCP_MAX_CONSECUTIVE_FAILURES: u32 = 3;
/// Default MCP idle timeout in seconds.
pub const DEFAULT_MCP_IDLE_TIMEOUT_SECS: u64 = 300;
/// Default memory decay half-life in days.
pub const DEFAULT_MEMORY_DECAY_HALF_LIFE_DAYS: u32 = 30;
/// Default memory retrieval boost factor.
pub const DEFAULT_MEMORY_RETRIEVAL_BOOST: f64 = 0.2;
/// Default memory prune threshold.
pub const DEFAULT_MEMORY_PRUNE_THRESHOLD: f64 = 0.1;
/// Default ACP agent CLI args.
pub const DEFAULT_ACP_ARGS: &str = "acp";
/// Default evolution check interval.
pub const DEFAULT_EVOLUTION_INTERVAL: &str = "2h";

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
    pub agent: AgentProcessConfig,
    /// Web reverse-proxy settings.
    pub web: WebConfig,
    /// Self-evolution loop settings.
    pub evolution: EvolutionConfig,
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
            max_connections: DEFAULT_DB_MAX_CONNECTIONS,
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
            url: DEFAULT_QDRANT_URL.to_owned(),
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
            base_url: DEFAULT_LLM_BASE_URL.to_owned(),
            api_key: None,
            model: DEFAULT_LLM_MODEL.to_owned(),
            max_tokens: DEFAULT_LLM_MAX_TOKENS,
            embedding_model: DEFAULT_EMBEDDING_MODEL.to_owned(),
            embedding_dim: DEFAULT_EMBEDDING_DIM,
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
            host: DEFAULT_HOST.to_owned(),
            port: DEFAULT_API_PORT,
            rate_limit_max_requests: DEFAULT_RATE_LIMIT_MAX_REQUESTS,
            rate_limit_window_secs: DEFAULT_RATE_LIMIT_WINDOW_SECS,
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
            session_ttl_seconds: DEFAULT_SESSION_TTL_SECONDS,
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
            url: DEFAULT_SEARXNG_URL.to_owned(),
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
            socket_path: PathBuf::from(DEFAULT_ADMIN_SOCKET_PATH),
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
            tick_interval_secs: DEFAULT_SCHEDULER_TICK_INTERVAL_SECS,
            agent_socket_path: PathBuf::from(DEFAULT_AGENT_SOCKET_PATH),
            socket_path: PathBuf::from(DEFAULT_SCHEDULER_SOCKET_PATH),
            max_concurrent_jobs: DEFAULT_SCHEDULER_MAX_CONCURRENT_JOBS,
            metrics_port: DEFAULT_SCHEDULER_METRICS_PORT,
            workspace_root: PathBuf::from(DEFAULT_WORKSPACE_ROOT),
            sandbox_profile: DEFAULT_SANDBOX_PROFILE.to_owned(),
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
            request_timeout_secs: DEFAULT_MCP_REQUEST_TIMEOUT_SECS,
            max_consecutive_failures: DEFAULT_MCP_MAX_CONSECUTIVE_FAILURES,
            idle_timeout_secs: DEFAULT_MCP_IDLE_TIMEOUT_SECS,
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
            args: vec![DEFAULT_ACP_ARGS.to_owned()],
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
            decay_half_life_days: DEFAULT_MEMORY_DECAY_HALF_LIFE_DAYS,
            retrieval_boost: DEFAULT_MEMORY_RETRIEVAL_BOOST,
            prune_threshold: DEFAULT_MEMORY_PRUNE_THRESHOLD,
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
            socket_path: PathBuf::from(DEFAULT_AGENT_SOCKET_PATH),
            metrics_port: DEFAULT_AGENT_METRICS_PORT,
            workspace_root: PathBuf::from(DEFAULT_WORKSPACE_ROOT),
            sandbox_profile: DEFAULT_SANDBOX_PROFILE.to_owned(),
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
            host: DEFAULT_HOST.to_owned(),
            port: DEFAULT_WEB_PORT,
            api_upstream_url: DEFAULT_WEB_API_UPSTREAM_URL.to_owned(),
            static_dir: None,
        }
    }
}

/// Self-evolution loop settings.
///
/// Only the check interval lives here — autonomy levels are DB-backed
/// in the `evolution_config` table.
///
/// Configurable via `[evolution]` TOML section or `SOBER_EVOLUTION_*` env vars.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct EvolutionConfig {
    /// How often the evolution check job runs (e.g. `"2h"`, `"30m"`).
    pub interval: String,
    /// Max recent conversations to scan per detection cycle.
    pub detection_conv_limit: i64,
    /// Max messages to sample per conversation during detection.
    pub detection_msg_limit: i64,
    /// Max tokens for the detection LLM call.
    pub detection_max_tokens: u32,
    /// Temperature for the detection LLM call.
    pub detection_temperature: f32,
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self {
            interval: DEFAULT_EVOLUTION_INTERVAL.to_owned(),
            detection_conv_limit: 20,
            detection_msg_limit: 50,
            detection_max_tokens: 4096,
            detection_temperature: 0.3,
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

        // evolution
        if let Some(v) = get_var("SOBER_EVOLUTION_INTERVAL") {
            self.evolution.interval = v;
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

        if let Some(ref key) = self.crypto.master_encryption_key
            && (key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(AppError::Validation(
                "invalid config: crypto.master_encryption_key — must be a 64-character hex string (256-bit key). Generate with: openssl rand -hex 32".to_owned(),
            ));
        }

        Ok(())
    }

    /// Loads configuration from environment variables.
    ///
    /// **Deprecated:** Use [`load()`](AppConfig::load) which also reads `config.toml`.
    #[deprecated(note = "use AppConfig::load() which also reads config.toml")]
    pub fn load_from_env() -> Result<Self, AppError> {
        dotenvy::dotenv().ok();
        Self::load_from(|key| std::env::var(key).ok())
    }

    /// Loads configuration from an arbitrary key-value source (for testing).
    ///
    /// Starts from defaults, applies the provided vars as overrides, then validates.
    pub fn load_from(get_var: impl Fn(&str) -> Option<String>) -> Result<Self, AppError> {
        let mut config = Self::default();
        config.apply_env_overrides(&get_var);
        config.validate()?;
        Ok(config)
    }
}

/// Wraps a key-lookup closure with helper methods for common patterns.
struct EnvSource<F: Fn(&str) -> Option<String>>(F);

impl<F: Fn(&str) -> Option<String>> EnvSource<F> {
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
            "SOBER_DATABASE_URL",
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
        assert!(matches!(err, AppError::Validation(msg) if msg.contains("database.url")));
    }

    #[test]
    fn production_environment() {
        let config = AppConfig::load_from(env_from(&[
            ("SOBER_DATABASE_URL", "postgres://test:test@localhost/test"),
            ("SOBER_ENV", "production"),
            ("SOBER_AUTH_SESSION_SECRET", "test-secret"),
        ]))
        .unwrap();
        assert_eq!(config.environment, Environment::Production);
    }

    #[test]
    fn invalid_port_is_ignored_and_default_kept() {
        // Invalid parse values in env overlay are silently ignored —
        // the default value is preserved.
        let config = AppConfig::load_from(env_from(&[
            ("SOBER_DATABASE_URL", "postgres://test:test@localhost/test"),
            ("SOBER_SERVER_PORT", "not_a_number"),
        ]))
        .unwrap();
        assert_eq!(config.server.port, 3000); // default kept
    }

    #[test]
    fn defaults_applied_correctly() {
        let config = AppConfig::load_from(env_from(&[(
            "SOBER_DATABASE_URL",
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
            "SOBER_DATABASE_URL",
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
            ("SOBER_DATABASE_URL", "postgres://test:test@localhost/test"),
            ("SOBER_LLM_API_KEY", "sk-test"),
            ("SOBER_AUTH_SESSION_SECRET", "my-secret"),
        ]))
        .unwrap();
        assert_eq!(config.llm.api_key.as_deref(), Some("sk-test"));
        assert_eq!(config.auth.session_secret.as_deref(), Some("my-secret"));
    }

    // ── Task 7: load_from with new env var names ────────────────────────────

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

    // ── Task 6: Validation ──────────────────────────────────────────────────

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
}
