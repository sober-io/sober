//! TOML configuration deserialization for sandbox policies.
//!
//! Matches the `[sandbox]` section in `.sober/config.toml` or
//! `~/.sober/config.toml`.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::error::SandboxError;
use crate::policy::{NetMode, SandboxPolicy, SandboxProfile};

/// Top-level `[sandbox]` configuration section.
#[derive(Debug, Clone, Deserialize)]
pub struct SandboxConfig {
    /// Base profile for this scope (default: `standard`).
    #[serde(default)]
    pub profile: SandboxProfile,

    /// Field-level overrides applied on top of the resolved profile.
    #[serde(default)]
    pub overrides: Option<SandboxOverrides>,

    /// User-defined named profiles.
    #[serde(default)]
    pub profiles: HashMap<String, SandboxPolicyConfig>,

    /// Per-tool sandbox overrides.
    #[serde(default)]
    pub tools: HashMap<String, ToolSandboxConfig>,
}

/// Field-level overrides that modify a resolved policy without replacing it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SandboxOverrides {
    /// Override read-only paths.
    pub fs_read: Option<Vec<PathBuf>>,
    /// Override read-write paths.
    pub fs_write: Option<Vec<PathBuf>>,
    /// Override denied paths.
    pub fs_deny: Option<Vec<PathBuf>>,
    /// Override allowed network domains.
    pub net_allow: Option<Vec<String>>,
    /// Explicit deny list (takes precedence over allow).
    pub net_deny: Option<Vec<String>>,
    /// Override spawn permission.
    pub process_spawn: Option<bool>,
    /// Override max execution time.
    pub max_execution_seconds: Option<u32>,
}

impl SandboxOverrides {
    /// Apply these overrides to a mutable policy.
    pub fn apply_to(&self, policy: &mut SandboxPolicy) {
        if let Some(ref paths) = self.fs_read {
            policy.fs_read = paths.clone();
        }
        if let Some(ref paths) = self.fs_write {
            policy.fs_write = paths.clone();
        }
        if let Some(ref paths) = self.fs_deny {
            policy.fs_deny = paths.clone();
        }
        if let Some(ref domains) = self.net_allow {
            // If net_allow is set, switch to AllowedDomains mode.
            policy.net_mode = NetMode::AllowedDomains(domains.clone());
        }
        if let Some(ref denied) = self.net_deny {
            // Filter denied domains from the allow list.
            if let NetMode::AllowedDomains(ref mut allowed) = policy.net_mode {
                allowed.retain(|d| !denied.contains(d));
            }
        }
        if let Some(spawn) = self.process_spawn {
            policy.allow_spawn = spawn;
        }
        if let Some(secs) = self.max_execution_seconds {
            policy.max_execution_seconds = secs;
        }
    }
}

/// TOML-friendly shape for a user-defined sandbox profile.
#[derive(Debug, Clone, Deserialize)]
pub struct SandboxPolicyConfig {
    /// Read-only filesystem paths.
    #[serde(default)]
    pub fs_read: Vec<PathBuf>,
    /// Read-write filesystem paths.
    #[serde(default)]
    pub fs_write: Vec<PathBuf>,
    /// Denied filesystem paths.
    #[serde(default)]
    pub fs_deny: Vec<PathBuf>,
    /// Allowed network domains (empty = no network).
    #[serde(default)]
    pub net_allow: Vec<String>,
    /// Whether process spawning is allowed.
    #[serde(default)]
    pub process_spawn: bool,
    /// Maximum execution time in seconds.
    #[serde(default = "default_max_execution_seconds")]
    pub max_execution_seconds: u32,
}

fn default_max_execution_seconds() -> u32 {
    60
}

impl SandboxPolicyConfig {
    /// Convert to a concrete [`SandboxPolicy`] with the given name.
    pub fn to_policy(&self, name: &str) -> SandboxPolicy {
        let net_mode = if self.net_allow.is_empty() {
            NetMode::None
        } else {
            NetMode::AllowedDomains(self.net_allow.clone())
        };

        SandboxPolicy {
            name: name.to_owned(),
            fs_read: self.fs_read.clone(),
            fs_write: self.fs_write.clone(),
            fs_deny: self.fs_deny.clone(),
            net_mode,
            max_execution_seconds: self.max_execution_seconds,
            allow_spawn: self.process_spawn,
        }
    }
}

/// Per-tool sandbox configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolSandboxConfig {
    /// Profile to use for this tool.
    #[serde(default)]
    pub profile: Option<SandboxProfile>,

    /// Field-level overrides for this tool.
    #[serde(flatten)]
    pub overrides: SandboxOverrides,
}

/// Parse a TOML string into a [`SandboxConfig`].
///
/// # Errors
///
/// Returns [`SandboxError::PolicyResolutionFailed`] if the TOML is malformed.
pub fn parse_config(toml_str: &str) -> Result<SandboxConfig, SandboxError> {
    toml::from_str(toml_str).map_err(|e| {
        SandboxError::PolicyResolutionFailed(format!("failed to parse sandbox config: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config() {
        let config: SandboxConfig = toml::from_str("").unwrap();
        assert_eq!(config.profile, SandboxProfile::Standard);
        assert!(config.profiles.is_empty());
        assert!(config.tools.is_empty());
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
profile = "locked-down"

[overrides]
fs_write = ["/workspace/output"]
net_allow = ["api.openai.com"]
max_execution_seconds = 45

[profiles.ci-runner]
fs_read = ["/workspace"]
fs_write = ["/workspace/build", "/tmp"]
net_allow = ["registry.npmjs.org", "github.com"]
process_spawn = true
max_execution_seconds = 120

[tools.web_search]
profile = "standard"
net_allow = ["*"]

[tools.code_runner]
profile = "locked-down"
"#;

        let config: SandboxConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.profile, SandboxProfile::LockedDown);

        let overrides = config.overrides.unwrap();
        assert_eq!(overrides.max_execution_seconds, Some(45));
        assert_eq!(overrides.net_allow, Some(vec!["api.openai.com".to_owned()]));

        let ci = &config.profiles["ci-runner"];
        assert!(ci.process_spawn);
        assert_eq!(ci.max_execution_seconds, 120);

        assert_eq!(
            config.tools["web_search"].profile,
            Some(SandboxProfile::Standard)
        );
        assert_eq!(
            config.tools["code_runner"].profile,
            Some(SandboxProfile::LockedDown)
        );
    }

    #[test]
    fn overrides_apply_to_policy() {
        let mut policy = SandboxProfile::Standard.resolve(&HashMap::new()).unwrap();

        let overrides = SandboxOverrides {
            fs_write: Some(vec![PathBuf::from("/output")]),
            net_allow: Some(vec!["example.com".into()]),
            max_execution_seconds: Some(90),
            process_spawn: Some(true),
            ..Default::default()
        };

        overrides.apply_to(&mut policy);

        assert_eq!(policy.fs_write, vec![PathBuf::from("/output")]);
        assert_eq!(
            policy.net_mode,
            NetMode::AllowedDomains(vec!["example.com".into()])
        );
        assert_eq!(policy.max_execution_seconds, 90);
        assert!(policy.allow_spawn);
    }

    #[test]
    fn net_deny_filters_allowed() {
        let mut policy = SandboxPolicy {
            name: "test".into(),
            fs_read: vec![],
            fs_write: vec![],
            fs_deny: vec![],
            net_mode: NetMode::AllowedDomains(vec![
                "good.com".into(),
                "bad.com".into(),
                "ok.com".into(),
            ]),
            max_execution_seconds: 60,
            allow_spawn: false,
        };

        let overrides = SandboxOverrides {
            net_deny: Some(vec!["bad.com".into()]),
            ..Default::default()
        };

        overrides.apply_to(&mut policy);

        assert_eq!(
            policy.net_mode,
            NetMode::AllowedDomains(vec!["good.com".into(), "ok.com".into()])
        );
    }

    #[test]
    fn policy_config_to_policy() {
        let config = SandboxPolicyConfig {
            fs_read: vec![PathBuf::from("/src")],
            fs_write: vec![PathBuf::from("/build")],
            fs_deny: vec![],
            net_allow: vec!["github.com".into()],
            process_spawn: true,
            max_execution_seconds: 120,
        };

        let policy = config.to_policy("my-profile");
        assert_eq!(policy.name, "my-profile");
        assert_eq!(
            policy.net_mode,
            NetMode::AllowedDomains(vec!["github.com".into()])
        );
        assert!(policy.allow_spawn);
    }

    #[test]
    fn parse_config_error() {
        let result = parse_config("invalid = [[[toml");
        assert!(result.is_err());
    }
}
