//! Integration tests for shell tool execution.
//!
//! Tests marked `#[ignore]` require bwrap to be installed on the system.
//! Run with `cargo test -- --ignored` to include them.

use std::collections::HashMap;
use std::path::PathBuf;

use sober_agent::tools::ShellTool;
use sober_core::PermissionMode;
use sober_core::types::tool::Tool;
use sober_sandbox::{CommandPolicy, SandboxProfile};

fn make_test_tool(workspace_home: PathBuf) -> ShellTool {
    let sandbox_policy = SandboxProfile::Standard
        .resolve(&HashMap::new())
        .expect("failed to resolve sandbox policy");
    ShellTool::new(
        CommandPolicy::default(),
        PermissionMode::Autonomous,
        workspace_home,
        sandbox_policy,
        false,
    )
}

#[tokio::test]
#[ignore = "requires bwrap"]
async fn shell_tool_executes_basic_command() {
    let dir = tempfile::tempdir().unwrap();
    let tool = make_test_tool(dir.path().to_path_buf());
    let result = tool
        .execute(serde_json::json!({"command": "echo hello"}))
        .await
        .unwrap();
    assert!(
        !result.is_error,
        "expected success, got: {}",
        result.content
    );
    assert!(result.content.contains("hello"));
}

#[tokio::test]
#[ignore = "requires bwrap"]
async fn shell_tool_captures_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let tool = make_test_tool(dir.path().to_path_buf());
    let result = tool
        .execute(serde_json::json!({"command": "ls /nonexistent_path_12345"}))
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("stderr:"));
}

#[tokio::test]
#[ignore = "requires bwrap"]
async fn shell_tool_respects_workdir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("subdir")).unwrap();
    let tool = make_test_tool(dir.path().to_path_buf());
    let result = tool
        .execute(serde_json::json!({"command": "pwd", "workdir": "subdir"}))
        .await
        .unwrap();
    assert!(
        !result.is_error,
        "expected success, got: {}",
        result.content
    );
    assert!(result.content.contains("subdir"));
}

#[tokio::test]
async fn shell_tool_denies_blocked_commands() {
    let dir = tempfile::tempdir().unwrap();
    let sandbox_policy = SandboxProfile::Standard
        .resolve(&HashMap::new())
        .expect("failed to resolve sandbox policy");
    let tool = ShellTool::new(
        CommandPolicy::with_denied(vec!["shutdown".to_string()]),
        PermissionMode::Autonomous,
        dir.path().to_path_buf(),
        sandbox_policy,
        false,
    );
    let result = tool
        .execute(serde_json::json!({"command": "shutdown -h now"}))
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("denied by system policy"));
}
