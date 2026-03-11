//! Filesystem operations for workspace directory management.

use std::path::Path;

use tokio::fs;

use crate::WorkspaceError;

const DEFAULT_CONFIG_TOML: &str = "\
# Workspace configuration for Sober agent.
# Uncomment and modify settings as needed.

# [llm]
# model = \"anthropic/claude-sonnet-4\"
# context_budget = 4096

# [style]
# tone = \"neutral\"
# commit_convention = \"conventional\"

# [sandbox]
# profile = \"standard\"            # locked_down | standard | unrestricted
# max_execution_seconds = 300
# network_mode = \"none\"           # none | allowed_domains | full
# allowed_domains = []

# [shell]
# permission_mode = \"policy_based\"  # interactive | policy_based | autonomous
# auto_snapshot = true
# max_snapshots = 10                 # oldest pruned when exceeded
#
# [shell.rules]
# \"docker compose\" = \"safe\"
# \"npm publish\" = \"dangerous\"
";

const DEFAULT_STATE_JSON: &str = r#"{"observations":[]}"#;

/// Initialize the `.sober/` directory structure inside a workspace root.
///
/// Creates the root directory and `.sober/` with subdirectories and template
/// files (`config.toml`, `state.json`) if they do not already exist.
pub async fn init_workspace_dir(workspace_root: &Path) -> Result<(), WorkspaceError> {
    let sober_dir = workspace_root.join(".sober");

    for subdir in ["proposals", "traces", "snapshots", "worktrees"] {
        fs::create_dir_all(sober_dir.join(subdir))
            .await
            .map_err(WorkspaceError::Filesystem)?;
    }

    let config_path = sober_dir.join("config.toml");
    if !config_path.exists() {
        fs::write(&config_path, DEFAULT_CONFIG_TOML)
            .await
            .map_err(WorkspaceError::Filesystem)?;
    }

    let state_path = sober_dir.join("state.json");
    if !state_path.exists() {
        fs::write(&state_path, DEFAULT_STATE_JSON)
            .await
            .map_err(WorkspaceError::Filesystem)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn init_workspace_dir_creates_structure() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("test-workspace");

        init_workspace_dir(&root).await.unwrap();

        assert!(root.join(".sober").is_dir());
        assert!(root.join(".sober/config.toml").is_file());
        assert!(root.join(".sober/state.json").is_file());
        assert!(root.join(".sober/proposals").is_dir());
        assert!(root.join(".sober/traces").is_dir());
        assert!(root.join(".sober/snapshots").is_dir());
        assert!(root.join(".sober/worktrees").is_dir());
    }

    #[tokio::test]
    async fn init_workspace_dir_default_config_is_valid_toml() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("test-workspace");

        init_workspace_dir(&root).await.unwrap();

        let content = tokio::fs::read_to_string(root.join(".sober/config.toml"))
            .await
            .unwrap();
        let config = sober_core::WorkspaceConfig::from_toml(&content).unwrap();
        assert!(config.llm.is_none());
    }

    #[tokio::test]
    async fn init_workspace_dir_default_state_is_valid_json() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("test-workspace");

        init_workspace_dir(&root).await.unwrap();

        let content = tokio::fs::read_to_string(root.join(".sober/state.json"))
            .await
            .unwrap();
        let state: sober_core::WorkspaceAgentState = serde_json::from_str(&content).unwrap();
        assert!(state.observations.is_empty());
    }

    #[tokio::test]
    async fn init_workspace_dir_idempotent() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("test-workspace");

        init_workspace_dir(&root).await.unwrap();
        // Write custom content to config
        tokio::fs::write(root.join(".sober/config.toml"), "[llm]\nmodel = \"test\"")
            .await
            .unwrap();

        // Re-init should not overwrite existing files
        init_workspace_dir(&root).await.unwrap();

        let content = tokio::fs::read_to_string(root.join(".sober/config.toml"))
            .await
            .unwrap();
        assert!(content.contains("model = \"test\""));
    }
}
