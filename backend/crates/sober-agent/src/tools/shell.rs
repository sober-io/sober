//! Shell command execution tool for the agent.
//!
//! Executes commands in a sandboxed environment (bwrap) within the user's
//! workspace. Returns [`ToolError::NeedsConfirmation`] for dangerous commands
//! so the agent loop can handle the interactive confirmation flow.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use serde::Deserialize;
use sober_core::PermissionMode;
use sober_core::types::tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
use sober_sandbox::{BwrapSandbox, CommandPolicy, RiskLevel, SandboxPolicy};
use sober_workspace::SnapshotManager;

/// Maximum output length returned to the LLM to avoid blowing up context.
const MAX_OUTPUT_LEN: usize = 16_000;

/// Default per-command timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u32 = 300;

/// Default maximum number of snapshots retained per workspace.
const DEFAULT_MAX_SNAPSHOTS: u32 = 10;

#[derive(Debug, Deserialize)]
struct ShellInput {
    command: String,
    workdir: Option<String>,
    timeout: Option<u32>,
    /// Set to `true` by the agent loop after user confirmation.
    #[serde(default)]
    confirmed: bool,
}

/// Thread-safe handle for reading and updating the permission mode at runtime.
pub type SharedPermissionMode = Arc<RwLock<PermissionMode>>;

/// Shell command execution tool.
///
/// Classification and sandboxed execution. When a command requires
/// confirmation, the tool returns [`ToolError::NeedsConfirmation`]
/// and the agent loop handles the interactive flow.
pub struct ShellTool {
    policy: CommandPolicy,
    permission_mode: SharedPermissionMode,
    workspace_home: PathBuf,
    sandbox_policy: SandboxPolicy,
    auto_snapshot: bool,
    snapshot_manager: Option<SnapshotManager>,
}

impl ShellTool {
    /// Create a new ShellTool with a shared permission mode.
    pub fn new(
        policy: CommandPolicy,
        permission_mode: SharedPermissionMode,
        workspace_home: PathBuf,
        sandbox_policy: SandboxPolicy,
        auto_snapshot: bool,
        snapshot_manager: Option<SnapshotManager>,
    ) -> Self {
        Self {
            policy,
            permission_mode,
            workspace_home,
            sandbox_policy,
            auto_snapshot,
            snapshot_manager,
        }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let input: ShellInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("invalid input: {e}")))?;

        if input.command.trim().is_empty() {
            return Err(ToolError::InvalidInput("command cannot be empty".into()));
        }

        // Check admin deny list
        if self.policy.is_denied(&input.command) {
            return Ok(ToolOutput {
                content: "Command denied by system policy.".to_string(),
                is_error: true,
            });
        }

        // Classify risk
        let risk = self.policy.classify(&input.command);

        // Read current permission mode (may be updated at runtime via gRPC).
        let permission_mode = *self
            .permission_mode
            .read()
            .expect("permission mode lock poisoned");

        // Check permission mode (skip if already confirmed)
        if !input.confirmed {
            let needs_confirmation = match (permission_mode, risk) {
                (PermissionMode::Autonomous, _) => false,
                (PermissionMode::PolicyBased, RiskLevel::Safe | RiskLevel::Moderate) => false,
                (PermissionMode::Interactive, _)
                | (PermissionMode::PolicyBased, RiskLevel::Dangerous) => true,
            };

            if needs_confirmation {
                return Err(ToolError::NeedsConfirmation {
                    confirm_id: uuid::Uuid::now_v7().to_string(),
                    command: input.command,
                    risk_level: format!("{risk:?}"),
                    reason: format!("Command classified as {risk:?}"),
                });
            }
        }

        // Snapshot before dangerous commands when auto_snapshot is enabled.
        if self.auto_snapshot
            && risk == RiskLevel::Dangerous
            && let Some(ref mgr) = self.snapshot_manager
        {
            match mgr.create(&self.workspace_home, "pre-dangerous").await {
                Ok(snap) => {
                    tracing::info!(path = %snap.path.display(), "auto-snapshot created");
                    // Best-effort prune old snapshots.
                    if let Err(e) = mgr.prune(DEFAULT_MAX_SNAPSHOTS).await {
                        tracing::warn!(error = %e, "snapshot prune failed");
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "auto-snapshot failed, proceeding anyway");
                }
            }
        }

        // Determine working directory
        let workdir = if let Some(ref wd) = input.workdir {
            self.workspace_home.join(wd)
        } else {
            self.workspace_home.clone()
        };

        // Build sandbox with workspace bind-mount
        let mut policy = self.sandbox_policy.clone();
        policy.fs_read.push(self.workspace_home.clone());
        policy.fs_write.push(self.workspace_home.clone());
        // System tool paths for read-only access
        for sys_path in ["/usr", "/bin", "/lib", "/lib64", "/etc/alternatives"] {
            let p = PathBuf::from(sys_path);
            if p.exists() && !policy.fs_read.contains(&p) {
                policy.fs_read.push(p);
            }
        }

        let timeout = input.timeout.unwrap_or(DEFAULT_TIMEOUT_SECS);
        policy.max_execution_seconds = timeout;

        let sandbox = BwrapSandbox::new(policy);

        // Execute via shell -c to support pipes, redirects, etc.
        let command = vec![
            "sh".to_string(),
            "-c".to_string(),
            format!("cd {} && {}", workdir.display(), input.command),
        ];

        let result = sandbox
            .execute(&command, &HashMap::new())
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("sandbox execution failed: {e}")))?;

        // Format output
        let mut output = String::new();
        output.push_str(&format!("Exit code: {}\n", result.exit_code));

        if !result.stdout.is_empty() {
            output.push_str("\nstdout:\n");
            output.push_str(&result.stdout);
        }

        if !result.stderr.is_empty() {
            output.push_str("\nstderr:\n");
            output.push_str(&result.stderr);
        }

        // Truncate if too long
        if output.len() > MAX_OUTPUT_LEN {
            output.truncate(MAX_OUTPUT_LEN);
            output.push_str("\n\n[output truncated]");
        }

        Ok(ToolOutput {
            content: output,
            is_error: result.exit_code != 0,
        })
    }
}

impl Tool for ShellTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "shell".to_string(),
            description: "Execute a shell command in the user's workspace. Use for building, \
                testing, file operations, installing tools, and running scripts. Commands run \
                in a sandboxed environment with the user's workspace mounted."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "workdir": {
                        "type": "string",
                        "description": "Working directory relative to workspace root (optional)"
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in seconds (optional, defaults to 300)"
                    }
                },
                "required": ["command"]
            }),
            context_modifying: false,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sober_sandbox::SandboxProfile;

    fn test_tool() -> ShellTool {
        ShellTool {
            policy: CommandPolicy::default(),
            permission_mode: Arc::new(RwLock::new(PermissionMode::Autonomous)),
            workspace_home: PathBuf::from("/tmp/test-workspace"),
            sandbox_policy: SandboxProfile::LockedDown.resolve(&HashMap::new()).unwrap(),
            auto_snapshot: false,
            snapshot_manager: None,
        }
    }

    fn test_tool_with_mode(mode: PermissionMode) -> ShellTool {
        ShellTool {
            permission_mode: Arc::new(RwLock::new(mode)),
            ..test_tool()
        }
    }

    #[test]
    fn shell_tool_metadata() {
        let tool = test_tool();
        let meta = tool.metadata();
        assert_eq!(meta.name, "shell");
        assert!(!meta.context_modifying);
        let required = meta.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("command")));
    }

    #[test]
    fn shell_tool_rejects_missing_command() {
        let tool = test_tool();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute_inner(serde_json::json!({})));
        assert!(result.is_err());
    }

    #[test]
    fn shell_tool_rejects_empty_command() {
        let tool = test_tool();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute_inner(serde_json::json!({"command": ""})));
        assert!(result.is_err());
    }

    #[test]
    fn shell_tool_denies_blocked_command() {
        let tool = ShellTool {
            policy: CommandPolicy::with_denied(vec!["shutdown".to_string()]),
            ..test_tool()
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(tool.execute_inner(serde_json::json!({"command": "shutdown -h now"})))
            .unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("denied by system policy"));
    }

    #[test]
    fn shell_tool_returns_needs_confirmation_for_dangerous() {
        let tool = test_tool_with_mode(PermissionMode::PolicyBased);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute_inner(serde_json::json!({"command": "rm -rf /"})));
        match result {
            Err(ToolError::NeedsConfirmation { command, .. }) => {
                assert_eq!(command, "rm -rf /");
            }
            other => panic!("expected NeedsConfirmation, got {other:?}"),
        }
    }

    #[test]
    fn shell_tool_skips_confirmation_when_confirmed() {
        let tool = test_tool_with_mode(PermissionMode::Interactive);
        let rt = tokio::runtime::Runtime::new().unwrap();
        // With confirmed=true, it should attempt execution (not return NeedsConfirmation)
        let result = rt.block_on(
            tool.execute_inner(serde_json::json!({"command": "echo hello", "confirmed": true})),
        );
        // It will either succeed or fail at sandbox execution, but NOT return NeedsConfirmation
        match result {
            Err(ToolError::NeedsConfirmation { .. }) => {
                panic!("should not need confirmation when confirmed=true");
            }
            _ => {} // any other result is fine (sandbox may fail in test env)
        }
    }

    #[test]
    fn shell_tool_respects_runtime_mode_change() {
        let shared_mode = Arc::new(RwLock::new(PermissionMode::Autonomous));
        let tool = ShellTool {
            permission_mode: Arc::clone(&shared_mode),
            ..test_tool()
        };
        let rt = tokio::runtime::Runtime::new().unwrap();

        // In Autonomous mode, dangerous commands should NOT need confirmation.
        let result = rt.block_on(tool.execute_inner(serde_json::json!({"command": "rm -rf /"})));
        assert!(
            !matches!(result, Err(ToolError::NeedsConfirmation { .. })),
            "Autonomous mode should not require confirmation"
        );

        // Switch to PolicyBased at runtime.
        *shared_mode.write().unwrap() = PermissionMode::PolicyBased;

        // Now the same dangerous command SHOULD need confirmation.
        let result = rt.block_on(tool.execute_inner(serde_json::json!({"command": "rm -rf /"})));
        assert!(
            matches!(result, Err(ToolError::NeedsConfirmation { .. })),
            "PolicyBased mode should require confirmation for dangerous commands"
        );
    }
}
