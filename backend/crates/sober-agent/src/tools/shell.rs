//! Shell command execution tool for the agent.
//!
//! Executes commands in a sandboxed environment (bwrap) within the user's
//! workspace. Supports permission modes and confirmation flow for sensitive
//! commands.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use sober_core::PermissionMode;
use sober_core::types::tool::{BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput};
use sober_sandbox::{BwrapSandbox, CommandPolicy, RiskLevel, SandboxPolicy};
use tokio::sync::oneshot;

/// Maximum output length returned to the LLM to avoid blowing up context.
const MAX_OUTPUT_LEN: usize = 16_000;

/// Default per-command timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u32 = 300;

#[derive(Debug, Deserialize)]
struct ShellInput {
    command: String,
    workdir: Option<String>,
    timeout: Option<u32>,
}

/// Payload sent to the frontend for confirmation.
#[derive(Debug, Clone)]
pub struct ConfirmPayload {
    /// Unique identifier for this confirmation request.
    pub confirm_id: String,
    /// The shell command to confirm.
    pub command: String,
    /// Risk classification of the command.
    pub risk_level: RiskLevel,
    /// Files/directories affected by the command.
    pub affects: Vec<String>,
    /// Human-readable reason why confirmation is needed.
    pub reason: String,
}

/// Callback type for requesting user confirmation.
/// Returns a oneshot receiver that resolves to the user's decision.
pub type ConfirmFn = Arc<dyn Fn(ConfirmPayload) -> oneshot::Receiver<bool> + Send + Sync>;

/// Shell command execution tool.
pub struct ShellTool {
    policy: CommandPolicy,
    permission_mode: PermissionMode,
    workspace_home: PathBuf,
    sandbox_policy: SandboxPolicy,
    confirm_fn: Option<ConfirmFn>,
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
        confirm_fn: Option<ConfirmFn>,
        auto_snapshot: bool,
    ) -> Self {
        Self {
            policy,
            permission_mode,
            workspace_home,
            sandbox_policy,
            confirm_fn,
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
            confirm_fn: None,
            auto_snapshot: false,
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

        // Check permission mode
        let needs_confirmation = match (self.permission_mode, risk) {
            (PermissionMode::Autonomous, _) => false,
            (PermissionMode::PolicyBased, RiskLevel::Safe | RiskLevel::Moderate) => false,
            (PermissionMode::Interactive, _)
            | (PermissionMode::PolicyBased, RiskLevel::Dangerous) => true,
        };

        if needs_confirmation {
            if let Some(ref confirm_fn) = self.confirm_fn {
                let payload = ConfirmPayload {
                    confirm_id: uuid::Uuid::now_v7().to_string(),
                    command: input.command.clone(),
                    risk_level: risk,
                    affects: Vec::new(), // TODO: analyze affected files
                    reason: format!("Command classified as {risk:?}"),
                };
                let rx = (confirm_fn)(payload);
                match tokio::time::timeout(
                    std::time::Duration::from_secs(u64::from(DEFAULT_TIMEOUT_SECS)),
                    rx,
                )
                .await
                {
                    Ok(Ok(true)) => {} // approved
                    Ok(Ok(false)) => {
                        return Ok(ToolOutput {
                            content: "Command denied by user.".to_string(),
                            is_error: false,
                        });
                    }
                    Ok(Err(_)) => {
                        return Ok(ToolOutput {
                            content: "Confirmation cancelled.".to_string(),
                            is_error: true,
                        });
                    }
                    Err(_) => {
                        return Ok(ToolOutput {
                            content: "Command timed out waiting for confirmation.".to_string(),
                            is_error: true,
                        });
                    }
                }
            } else {
                return Ok(ToolOutput {
                    content: format!(
                        "Command requires confirmation but no confirmation handler is available. Risk level: {risk:?}"
                    ),
                    is_error: true,
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

        let result = sandbox
            .execute(&command, &HashMap::new())
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("sandbox error: {e}")))?;

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
            context_modifying: true,
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
        assert!(meta.context_modifying);
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
            confirm_fn: None,
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
    fn shell_tool_requires_confirm_for_dangerous_in_policy_mode() {
        let tool = ShellTool {
            policy: CommandPolicy::default(),
            permission_mode: PermissionMode::PolicyBased,
            workspace_home: PathBuf::from("/tmp/test"),
            sandbox_policy: sober_sandbox::SandboxProfile::LockedDown
                .resolve(&std::collections::HashMap::new())
                .unwrap(),
            confirm_fn: None, // No confirm handler
            auto_snapshot: false,
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(tool.execute_inner(serde_json::json!({"command": "rm -rf /"})))
            .unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("requires confirmation"));
    }
}
