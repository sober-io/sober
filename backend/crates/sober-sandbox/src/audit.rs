//! Sandbox audit entry types.
//!
//! Every sandboxed execution produces a [`SandboxAuditEntry`] that captures
//! the resolved policy, command, duration, and outcome. The calling crate
//! is responsible for persisting these entries (e.g., to PostgreSQL).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bwrap::SandboxResult;
use crate::policy::SandboxPolicy;

/// Audit record for a single sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxAuditEntry {
    /// Unique execution identifier.
    pub execution_id: Uuid,
    /// When the execution started.
    pub timestamp: DateTime<Utc>,
    /// Workspace context (if applicable).
    pub workspace_id: Option<Uuid>,
    /// User who triggered the execution (if applicable).
    pub user_id: Option<Uuid>,
    /// Snapshot of the resolved policy at execution time.
    pub policy: SandboxPolicy,
    /// The command that was executed.
    pub command: Vec<String>,
    /// What triggered this execution.
    pub trigger: ExecutionTrigger,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
    /// Process exit code (if completed).
    pub exit_code: Option<i32>,
    /// Domains denied by the network proxy.
    pub denied_network_requests: Vec<String>,
    /// How the execution ended.
    pub outcome: ExecutionOutcome,
}

/// What triggered the sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionTrigger {
    /// Triggered by the agent autonomously.
    Agent,
    /// Triggered by a specific tool.
    Tool(String),
    /// Triggered by a user action.
    User,
    /// Triggered by the scheduler.
    Scheduler,
}

/// How a sandbox execution ended.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionOutcome {
    /// Process exited with code 0.
    Success,
    /// Process exceeded its time limit.
    Timeout,
    /// Process was killed (SIGKILL after grace period).
    Killed,
    /// Process exited with a non-zero code or other error.
    Error(String),
}

impl SandboxAuditEntry {
    /// Construct an audit entry from execution inputs and result.
    pub fn from_result(
        policy: SandboxPolicy,
        command: Vec<String>,
        trigger: ExecutionTrigger,
        result: &SandboxResult,
    ) -> Self {
        let outcome = if result.exit_code == 0 {
            ExecutionOutcome::Success
        } else {
            ExecutionOutcome::Error(format!("exit code {}", result.exit_code))
        };

        Self {
            execution_id: Uuid::now_v7(),
            timestamp: Utc::now(),
            workspace_id: None,
            user_id: None,
            policy,
            command,
            trigger,
            duration_ms: result.duration_ms,
            exit_code: Some(result.exit_code),
            denied_network_requests: result.denied_network_requests.clone(),
            outcome,
        }
    }

    /// Construct an audit entry for a timed-out execution.
    pub fn from_timeout(
        policy: SandboxPolicy,
        command: Vec<String>,
        trigger: ExecutionTrigger,
        duration_ms: u64,
    ) -> Self {
        Self {
            execution_id: Uuid::now_v7(),
            timestamp: Utc::now(),
            workspace_id: None,
            user_id: None,
            policy,
            command,
            trigger,
            duration_ms,
            exit_code: None,
            denied_network_requests: vec![],
            outcome: ExecutionOutcome::Timeout,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::NetMode;
    use std::path::PathBuf;

    fn test_policy() -> SandboxPolicy {
        SandboxPolicy {
            name: "test".into(),
            fs_read: vec![],
            fs_write: vec![PathBuf::from("/tmp")],
            fs_deny: vec![],
            net_mode: NetMode::None,
            max_execution_seconds: 30,
            allow_spawn: false,
        }
    }

    #[test]
    fn from_result_success() {
        let result = SandboxResult {
            exit_code: 0,
            stdout: "hello".into(),
            stderr: String::new(),
            duration_ms: 150,
            denied_network_requests: vec![],
        };

        let entry = SandboxAuditEntry::from_result(
            test_policy(),
            vec!["echo".into(), "hello".into()],
            ExecutionTrigger::Agent,
            &result,
        );

        assert_eq!(entry.outcome, ExecutionOutcome::Success);
        assert_eq!(entry.exit_code, Some(0));
        assert_eq!(entry.duration_ms, 150);
    }

    #[test]
    fn from_result_error() {
        let result = SandboxResult {
            exit_code: 1,
            stdout: String::new(),
            stderr: "error".into(),
            duration_ms: 200,
            denied_network_requests: vec!["evil.com".into()],
        };

        let entry = SandboxAuditEntry::from_result(
            test_policy(),
            vec!["bad_cmd".into()],
            ExecutionTrigger::Tool("code_runner".into()),
            &result,
        );

        assert!(matches!(entry.outcome, ExecutionOutcome::Error(_)));
        assert_eq!(entry.denied_network_requests, vec!["evil.com"]);
        assert_eq!(entry.trigger, ExecutionTrigger::Tool("code_runner".into()));
    }

    #[test]
    fn from_timeout() {
        let entry = SandboxAuditEntry::from_timeout(
            test_policy(),
            vec!["sleep".into(), "60".into()],
            ExecutionTrigger::User,
            30_000,
        );

        assert_eq!(entry.outcome, ExecutionOutcome::Timeout);
        assert_eq!(entry.exit_code, None);
        assert_eq!(entry.duration_ms, 30_000);
    }

    #[test]
    fn serde_roundtrip() {
        let result = SandboxResult {
            exit_code: 0,
            stdout: "ok".into(),
            stderr: String::new(),
            duration_ms: 100,
            denied_network_requests: vec![],
        };

        let entry = SandboxAuditEntry::from_result(
            test_policy(),
            vec!["echo".into()],
            ExecutionTrigger::Scheduler,
            &result,
        );

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: SandboxAuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.outcome, ExecutionOutcome::Success);
        assert_eq!(deserialized.trigger, ExecutionTrigger::Scheduler);
    }
}
