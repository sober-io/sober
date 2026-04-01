//! Shared test helpers for all downstream crates.
//!
//! Enable with `sober-core = { ..., features = ["test-utils"] }` in
//! `[dev-dependencies]`.

use crate::config::{
    AdminConfig, AppConfig, AuthConfig, DatabaseConfig, Environment, LlmConfig, McpConfig,
    MemoryConfig, QdrantConfig, SchedulerConfig, SearxngConfig, ServerConfig,
};
use std::path::PathBuf;

/// Creates an [`AppConfig`] with sensible test defaults.
///
/// No environment variables are read — all values are hardcoded for
/// deterministic test behavior.
#[must_use]
pub fn test_config() -> AppConfig {
    AppConfig {
        database: DatabaseConfig {
            url: "postgres://sober:sober@localhost:5432/sober_test".to_owned(),
            max_connections: 2,
        },
        qdrant: QdrantConfig {
            url: "http://localhost:6334".to_owned(),
            api_key: None,
        },
        llm: LlmConfig {
            base_url: "http://localhost:11434/v1".to_owned(),
            api_key: Some("test-key".to_owned()),
            model: "test-model".to_owned(),
            max_tokens: 1024,
            embedding_model: "test-embed".to_owned(),
            embedding_dim: 1536,
            vision: false,
        },
        server: ServerConfig {
            host: "127.0.0.1".to_owned(),
            port: 0, // OS-assigned port
        },
        auth: AuthConfig {
            session_secret: Some("test-secret-do-not-use-in-production".to_owned()),
            session_ttl_seconds: 3600,
        },
        searxng: SearxngConfig {
            url: "http://localhost:8080".to_owned(),
        },
        admin: AdminConfig {
            socket_path: PathBuf::from("/tmp/sober-test-admin.sock"),
        },
        scheduler: SchedulerConfig {
            tick_interval_secs: 1,
            agent_socket_path: PathBuf::from("/tmp/sober-test-agent.sock"),
            socket_path: PathBuf::from("/tmp/sober-test-scheduler.sock"),
            max_concurrent_jobs: 2,
        },
        mcp: McpConfig {
            request_timeout_secs: 5,
            max_consecutive_failures: 1,
            idle_timeout_secs: 10,
        },
        memory: MemoryConfig {
            decay_half_life_days: 7,
            retrieval_boost: 0.1,
            prune_threshold: 0.05,
        },
        environment: Environment::Development,
    }
}

// TODO(plan-008): MockLlmEngine — mock LLM engine for testing
// TODO(plan-012): MockGrpcServer — mock gRPC server for testing
