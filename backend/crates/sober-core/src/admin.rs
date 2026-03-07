//! Admin protocol types for CLI ↔ server communication.
//!
//! These types are shared between `sober-cli` (`soberctl`) and the service
//! processes (`sober-api`, `sober-scheduler`) for communication over Unix
//! domain sockets.

use serde::{Deserialize, Serialize};

use crate::types::ScopeId;

/// Commands that can be sent from `soberctl` to a running service.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AdminCommand {
    /// Health check ping.
    Ping,
    /// Query the running agent's status.
    AgentStatus,
    /// Query the task queue depth and state.
    TaskQueueStatus,
    /// Trigger memory pruning, optionally for a specific scope.
    PruneMemory {
        /// If `Some`, prune only this scope. If `None`, prune all scopes.
        scope_id: Option<ScopeId>,
    },
    /// Initiate service shutdown.
    Shutdown {
        /// If `true`, drain active requests before stopping.
        graceful: bool,
    },
}

/// Responses sent from a service back to `soberctl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AdminResponse {
    /// Response to a [`AdminCommand::Ping`].
    Pong,
    /// Status information as a JSON payload.
    Status {
        /// The status data.
        payload: serde_json::Value,
    },
    /// Command accepted successfully.
    Ok,
    /// Command failed with a reason.
    Error {
        /// The error message.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_serde_roundtrip() {
        let commands = vec![
            AdminCommand::Ping,
            AdminCommand::AgentStatus,
            AdminCommand::TaskQueueStatus,
            AdminCommand::PruneMemory { scope_id: None },
            AdminCommand::PruneMemory {
                scope_id: Some(ScopeId::new()),
            },
            AdminCommand::Shutdown { graceful: true },
            AdminCommand::Shutdown { graceful: false },
        ];

        for cmd in commands {
            let json = serde_json::to_string(&cmd).unwrap();
            let deserialized: AdminCommand = serde_json::from_str(&json).unwrap();
            // Verify the type tag is present
            let value: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert!(value["type"].is_string());
            // Verify roundtrip produces valid JSON
            let _ = serde_json::to_string(&deserialized).unwrap();
        }
    }

    #[test]
    fn response_serde_roundtrip() {
        let responses = vec![
            AdminResponse::Pong,
            AdminResponse::Status {
                payload: serde_json::json!({"uptime": 3600}),
            },
            AdminResponse::Ok,
            AdminResponse::Error {
                message: "not ready".into(),
            },
        ];

        for resp in responses {
            let json = serde_json::to_string(&resp).unwrap();
            let deserialized: AdminResponse = serde_json::from_str(&json).unwrap();
            let _ = serde_json::to_string(&deserialized).unwrap();
        }
    }

    #[test]
    fn ping_serializes_as_expected() {
        let json = serde_json::to_string(&AdminCommand::Ping).unwrap();
        assert_eq!(json, r#"{"type":"ping"}"#);
    }

    #[test]
    fn pong_serializes_as_expected() {
        let json = serde_json::to_string(&AdminResponse::Pong).unwrap();
        assert_eq!(json, r#"{"type":"pong"}"#);
    }
}
