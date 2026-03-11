//! Workspace configuration and agent state types.
//!
//! `.sober/config.toml` — user-edited, agent reads only.
//! `.sober/state.json` — agent-managed, user should not edit.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// User-editable workspace configuration (parsed from `.sober/config.toml`).
///
/// Additional sections (`[sandbox]`, `[shell]`) will be added by plan 022
/// (shell execution). Unknown TOML sections are silently ignored.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// LLM provider settings.
    pub llm: Option<WorkspaceLlmConfig>,
    /// Style/convention settings.
    pub style: Option<WorkspaceStyleConfig>,
    /// Sandbox execution policy.
    pub sandbox: Option<WorkspaceSandboxConfig>,
    /// Shell execution settings.
    pub shell: Option<WorkspaceShellConfig>,
}

/// LLM configuration for a workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceLlmConfig {
    /// Model identifier (e.g. "anthropic/claude-sonnet-4").
    pub model: String,
    /// Maximum context budget in tokens.
    pub context_budget: Option<usize>,
}

/// Style and convention settings for a workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceStyleConfig {
    /// Communication tone (e.g. "neutral", "formal").
    pub tone: Option<String>,
    /// Commit message convention (e.g. "conventional").
    pub commit_convention: Option<String>,
}

/// How much autonomy the agent has when executing shell commands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Every command requires explicit user approval.
    Interactive,
    /// Safe/moderate commands auto-approve; dangerous commands require approval.
    #[default]
    PolicyBased,
    /// All commands auto-approve within sandbox constraints.
    Autonomous,
}

/// Sandbox execution policy for this workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSandboxConfig {
    /// Sandbox profile name.
    #[serde(rename = "profile")]
    pub permission_profile: Option<String>,
    /// Per-command timeout in seconds.
    pub max_execution_seconds: Option<u32>,
    /// Network access mode.
    pub network_mode: Option<String>,
    /// Allowed domains when network_mode = "allowed_domains".
    pub allowed_domains: Option<Vec<String>>,
}

/// Shell execution settings for agent commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceShellConfig {
    /// Permission mode for command execution.
    #[serde(default)]
    pub permission_mode: PermissionMode,
    /// Auto-snapshot before dangerous commands.
    pub auto_snapshot: Option<bool>,
    /// Maximum snapshots retained.
    pub max_snapshots: Option<u32>,
    /// Per-command risk level overrides.
    pub rules: Option<HashMap<String, String>>,
}

impl WorkspaceConfig {
    /// Parse a workspace config from TOML text.
    /// Empty or missing sections produce `None` fields.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        toml::from_str(text)
    }
}

/// Agent-managed workspace state (serialized to `.sober/state.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceAgentState {
    /// Learned observations about this workspace.
    pub observations: Vec<Observation>,
}

/// A single observation the agent has recorded about a workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    /// What was observed (e.g. "commit_convention").
    pub key: String,
    /// The observed value.
    pub value: String,
    /// Confidence level (0.0–1.0).
    pub confidence: f32,
    /// ISO 8601 timestamp of when this was observed.
    pub observed_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_config() {
        let config = WorkspaceConfig::from_toml("").unwrap();
        assert!(config.llm.is_none());
        assert!(config.style.is_none());
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
[llm]
model = "anthropic/claude-sonnet-4"
context_budget = 2048

[style]
tone = "formal"
commit_convention = "conventional"
"#;
        let config = WorkspaceConfig::from_toml(toml).unwrap();
        assert_eq!(
            config.llm.as_ref().unwrap().model,
            "anthropic/claude-sonnet-4"
        );
        assert_eq!(config.llm.as_ref().unwrap().context_budget, Some(2048));
        assert_eq!(
            config.style.as_ref().unwrap().tone.as_deref(),
            Some("formal")
        );
    }

    #[test]
    fn permission_mode_serde_roundtrip() {
        let variants = [
            PermissionMode::Interactive,
            PermissionMode::PolicyBased,
            PermissionMode::Autonomous,
        ];
        for v in variants {
            let json = serde_json::to_string(&v).unwrap();
            let back: PermissionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(v, back);
        }
    }

    #[test]
    fn permission_mode_default_is_policy_based() {
        assert_eq!(PermissionMode::default(), PermissionMode::PolicyBased);
    }

    #[test]
    fn parse_config_with_shell_section() {
        let toml = r#"
[shell]
permission_mode = "autonomous"
auto_snapshot = false

[shell.rules]
"docker compose" = "safe"
"#;
        let config = WorkspaceConfig::from_toml(toml).unwrap();
        let shell = config.shell.unwrap();
        assert_eq!(shell.permission_mode, PermissionMode::Autonomous);
        assert_eq!(shell.auto_snapshot, Some(false));
        assert_eq!(shell.rules.unwrap().get("docker compose").unwrap(), "safe");
    }

    #[test]
    fn parse_workspace_state_json() {
        let state = WorkspaceAgentState::default();
        let json = serde_json::to_string(&state).unwrap();
        let back: WorkspaceAgentState = serde_json::from_str(&json).unwrap();
        assert_eq!(state.observations.len(), back.observations.len());
    }
}
