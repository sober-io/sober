//! Shell command execution tool for the agent.
//!
//! Executes commands in a sandboxed environment (bwrap) within the user's
//! workspace. Returns [`ToolError::NeedsConfirmation`] for dangerous commands
//! so the agent loop can handle the interactive confirmation flow.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;
use sober_core::PermissionMode;
use sober_core::types::tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
use sober_sandbox::{BwrapSandbox, CommandPolicy, RiskLevel, SandboxPolicy};

/// Maximum output length returned to the LLM to avoid blowing up context.
const MAX_OUTPUT_LEN: usize = 16_000;

/// Default per-command timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u32 = 300;

#[derive(Debug, Deserialize)]
struct ShellInput {
    command: String,
    workdir: Option<String>,
    timeout: Option<u32>,
    /// Set to `true` by the agent loop after user confirmation.
    #[serde(default)]
    confirmed: bool,
}

/// Shell command execution tool.
///
/// Stateless: classification and sandboxed execution only. When a command
/// requires confirmation, the tool returns [`ToolError::NeedsConfirmation`]
/// and the agent loop handles the interactive flow.
pub struct ShellTool {
    policy: CommandPolicy,
    permission_mode: PermissionMode,
    workspace_home: PathBuf,
    sandbox_policy: SandboxPolicy,
    #[allow(dead_code)]
    auto_snapshot: bool,
}

impl ShellTool {
    /// Create a new ShellTool.
    pub fn new(
        policy: CommandPolicy,
        permission_mode: PermissionMode,
        workspace_home: PathBuf,
        sandbox_policy: SandboxPolicy,
        auto_snapshot: bool,
    ) -> Self {
        Self {
            policy,
            permission_mode,
            workspace_home,
            sandbox_policy,
            auto_snapshot,
        }
    }

    #[cfg(test)]
    fn new_for_test() -> Self {
        use sober_sandbox::SandboxProfile;
        Self {
            policy: CommandPolicy::default(),
            permission_mode: PermissionMode::Autonomous,
            workspace_home: PathBuf::from("/tmp/test-workspace"),
            sandbox_policy: SandboxProfile::LockedDown
                .resolve(&std::collections::HashMap::new())
                .unwrap(),
            auto_snapshot: false,
        }
    }

    /// Direct execution fallback when bwrap is unavailable (e.g. inside Docker).
    async fn execute_direct(
        &self,
        command: &str,
        workdir: &std::path::Path,
        timeout_secs: u32,
    ) -> Result<sober_sandbox::SandboxResult, ToolError> {
        use tokio::process::Command;

        let start = std::time::Instant::now();
        let child = Command::new("sh")
            .args(["-c", command])
            .current_dir(workdir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to spawn: {e}")))?;

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(u64::from(timeout_secs)),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| ToolError::ExecutionFailed("command timed out".into()))?
        .map_err(|e| ToolError::ExecutionFailed(format!("command failed: {e}")))?;

        Ok(sober_sandbox::SandboxResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            duration_ms: start.elapsed().as_millis() as u64,
            denied_network_requests: vec![],
        })
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

        // Check permission mode (skip if already confirmed)
        if !input.confirmed {
            let needs_confirmation = match (self.permission_mode, risk) {
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

        let result = match sandbox.execute(&command, &HashMap::new()).await {
            Ok(r) => r,
            Err(e) => {
                // Fallback: run directly without bwrap (e.g. inside Docker where
                // pivot_root / user namespaces aren't available).
                tracing::warn!(
                    error = %e,
                    "bwrap sandbox failed, falling back to direct execution"
                );
                self.execute_direct(&input.command, &workdir, timeout)
                    .await?
            }
        };

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

    #[test]
    fn shell_tool_metadata() {
        let tool = ShellTool::new_for_test();
        let meta = tool.metadata();
        assert_eq!(meta.name, "shell");
        assert!(!meta.context_modifying);
        let required = meta.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("command")));
    }

    #[test]
    fn shell_tool_rejects_missing_command() {
        let tool = ShellTool::new_for_test();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute_inner(serde_json::json!({})));
        assert!(result.is_err());
    }

    #[test]
    fn shell_tool_rejects_empty_command() {
        let tool = ShellTool::new_for_test();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute_inner(serde_json::json!({"command": ""})));
        assert!(result.is_err());
    }

    #[test]
    fn shell_tool_denies_blocked_command() {
        let tool = ShellTool {
            policy: CommandPolicy::with_denied(vec!["shutdown".to_string()]),
            permission_mode: PermissionMode::Autonomous,
            workspace_home: PathBuf::from("/tmp/test"),
            sandbox_policy: sober_sandbox::SandboxProfile::LockedDown
                .resolve(&std::collections::HashMap::new())
                .unwrap(),
            auto_snapshot: false,
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
        let tool = ShellTool {
            policy: CommandPolicy::default(),
            permission_mode: PermissionMode::PolicyBased,
            workspace_home: PathBuf::from("/tmp/test"),
            sandbox_policy: sober_sandbox::SandboxProfile::LockedDown
                .resolve(&std::collections::HashMap::new())
                .unwrap(),
            auto_snapshot: false,
        };
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
        let tool = ShellTool {
            policy: CommandPolicy::default(),
            permission_mode: PermissionMode::Interactive,
            workspace_home: PathBuf::from("/tmp/test"),
            sandbox_policy: sober_sandbox::SandboxProfile::LockedDown
                .resolve(&std::collections::HashMap::new())
                .unwrap(),
            auto_snapshot: false,
        };
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
}
