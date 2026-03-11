//! Workspace configuration and agent state types.
//!
//! `.sober/config.toml` — user-edited, agent reads only.
//! `.sober/state.json` — agent-managed, user should not edit.

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
    fn parse_workspace_state_json() {
        let state = WorkspaceAgentState::default();
        let json = serde_json::to_string(&state).unwrap();
        let back: WorkspaceAgentState = serde_json::from_str(&json).unwrap();
        assert_eq!(state.observations.len(), back.observations.len());
    }
}
