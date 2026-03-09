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
#[derive(Debug, Clone)]
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
}

/// Runtime environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    /// Local development with pretty logging.
    Development,
    /// Production with JSON logging.
    Production,
}

/// PostgreSQL connection settings.
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// PostgreSQL connection URL.
    pub url: String,
    /// Maximum number of connections in the pool.
    pub max_connections: u32,
}

/// Qdrant vector database settings.
#[derive(Debug, Clone)]
pub struct QdrantConfig {
    /// Qdrant gRPC/HTTP URL.
    pub url: String,
    /// Optional API key for authentication.
    pub api_key: Option<String>,
}

/// LLM provider settings.
#[derive(Debug, Clone)]
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
}

/// HTTP server settings.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Bind address (e.g. `0.0.0.0`).
    pub host: String,
    /// Bind port.
    pub port: u16,
}

/// Authentication and session settings.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Secret used for session token signing (required in production).
    pub session_secret: Option<String>,
    /// Session time-to-live in seconds.
    pub session_ttl_seconds: u64,
}

/// SearXNG search engine settings.
#[derive(Debug, Clone)]
pub struct SearxngConfig {
    /// SearXNG instance URL.
    pub url: String,
}

/// Admin Unix socket settings.
#[derive(Debug, Clone)]
pub struct AdminConfig {
    /// Path to the admin Unix domain socket.
    pub socket_path: PathBuf,
}

/// Scheduler settings.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Tick interval in seconds.
    pub tick_interval_secs: u64,
    /// Path to the agent gRPC socket.
    pub agent_socket_path: PathBuf,
    /// Path to the scheduler's own admin socket.
    pub admin_socket_path: PathBuf,
    /// Maximum number of concurrent jobs.
    pub max_concurrent_jobs: u32,
}

/// MCP server connection settings.
#[derive(Debug, Clone)]
pub struct McpConfig {
    /// Request timeout in seconds.
    pub request_timeout_secs: u64,
    /// Maximum consecutive failures before marking a server unhealthy.
    pub max_consecutive_failures: u32,
    /// Idle timeout in seconds before disconnecting.
    pub idle_timeout_secs: u64,
}

/// ACP (Agent Client Protocol) agent settings.
#[derive(Debug, Clone)]
pub struct AcpAgentConfig {
    /// Display name for the agent.
    pub name: String,
    /// Binary to spawn (e.g. `"claude"`, `"kimi"`).
    pub command: String,
    /// CLI arguments (e.g. `["acp"]`).
    pub args: Vec<String>,
}

/// Memory system tuning parameters.
#[derive(Debug, Clone)]
pub struct MemoryConfig {
    /// Half-life for importance score decay, in days.
    pub decay_half_life_days: u32,
    /// Boost factor applied to recently retrieved memories.
    pub retrieval_boost: f64,
    /// Importance threshold below which memories are pruned.
    pub prune_threshold: f64,
}

/// Envelope encryption settings for secrets management.
#[derive(Debug, Clone)]
pub struct CryptoConfig {
    /// Hex-encoded 256-bit master encryption key for envelope encryption.
    ///
    /// Required when secrets management is enabled. Generate with:
    /// `openssl rand -hex 32`
    pub master_encryption_key: Option<String>,
}

impl AppConfig {
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
            },
            server: ServerConfig {
                host: env.or("HOST", "0.0.0.0"),
                port: env.parse("PORT", 3000)?,
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
                tick_interval_secs: env.parse("SCHEDULER_TICK_INTERVAL_SECS", 60)?,
                agent_socket_path: PathBuf::from(
                    env.or("SCHEDULER_AGENT_SOCKET_PATH", "/run/sober/agent.sock"),
                ),
                admin_socket_path: PathBuf::from(env.or(
                    "SCHEDULER_ADMIN_SOCKET_PATH",
                    "/run/sober/scheduler-admin.sock",
                )),
                max_concurrent_jobs: env.parse("SCHEDULER_MAX_CONCURRENT_JOBS", 10)?,
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(config.scheduler.tick_interval_secs, 60);
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
}
