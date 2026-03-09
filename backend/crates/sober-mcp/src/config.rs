//! MCP client configuration.

use serde::{Deserialize, Serialize};

/// Configuration for MCP client behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// Timeout for individual MCP requests in seconds.
    pub request_timeout_secs: u64,
    /// Maximum consecutive failures before marking a server as unavailable.
    pub max_consecutive_failures: u32,
    /// Idle timeout before disconnecting from an MCP server (seconds).
    pub idle_timeout_secs: u64,
    /// Cooldown period after a server failure before attempting reconnection (seconds).
    pub restart_cooldown_secs: u64,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            request_timeout_secs: 30,
            max_consecutive_failures: 3,
            idle_timeout_secs: 300,
            restart_cooldown_secs: 5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = McpConfig::default();
        assert_eq!(config.request_timeout_secs, 30);
        assert_eq!(config.max_consecutive_failures, 3);
        assert_eq!(config.idle_timeout_secs, 300);
        assert_eq!(config.restart_cooldown_secs, 5);
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = McpConfig {
            request_timeout_secs: 60,
            max_consecutive_failures: 5,
            idle_timeout_secs: 600,
            restart_cooldown_secs: 10,
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let deserialized: McpConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.request_timeout_secs, 60);
        assert_eq!(deserialized.max_consecutive_failures, 5);
        assert_eq!(deserialized.idle_timeout_secs, 600);
        assert_eq!(deserialized.restart_cooldown_secs, 10);
    }
}
