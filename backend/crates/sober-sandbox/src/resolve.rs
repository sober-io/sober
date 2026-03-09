//! Policy resolution chain.
//!
//! Resolves a concrete [`SandboxPolicy`] from the layered config chain:
//! tool override -> workspace config -> user config -> system default.

use std::collections::HashMap;

use crate::config::SandboxConfig;
use crate::error::SandboxError;
use crate::policy::{SandboxPolicy, SandboxProfile};

/// Resolve a [`SandboxPolicy`] from the layered config chain.
///
/// Resolution order (most specific wins):
/// 1. If `tool_name` is set and the workspace config has a matching
///    `tools.<name>` entry, use that tool's profile as the base.
/// 2. Otherwise, use the workspace config's `profile` field.
/// 3. If no workspace config, fall back to user config.
/// 4. If no user config, fall back to system default (`Standard`).
/// 5. Resolve the profile to a concrete policy.
/// 6. Apply overrides: workspace-level first, then tool-level.
pub fn resolve_policy(
    tool_name: Option<&str>,
    workspace_config: Option<&SandboxConfig>,
    user_config: Option<&SandboxConfig>,
) -> Result<SandboxPolicy, SandboxError> {
    // Build the custom profiles registry from all config sources.
    let mut custom_profiles: HashMap<String, SandboxPolicy> = HashMap::new();

    // User-level custom profiles (lower priority).
    if let Some(user) = user_config {
        for (name, cfg) in &user.profiles {
            custom_profiles.insert(name.clone(), cfg.to_policy(name));
        }
    }

    // Workspace-level custom profiles (higher priority, overwrite user).
    if let Some(ws) = workspace_config {
        for (name, cfg) in &ws.profiles {
            custom_profiles.insert(name.clone(), cfg.to_policy(name));
        }
    }

    // Determine the base profile and which config layer it came from.
    let (base_profile, active_config) =
        resolve_base_profile(tool_name, workspace_config, user_config);

    // Resolve the profile to a concrete policy.
    let mut policy = base_profile.resolve(&custom_profiles)?;

    // Apply workspace-level overrides.
    if let Some(ws) = active_config
        && let Some(ref overrides) = ws.overrides
    {
        overrides.apply_to(&mut policy);
    }

    // Apply tool-level overrides (most specific).
    if let Some(name) = tool_name
        && let Some(ws) = workspace_config
        && let Some(tool_cfg) = ws.tools.get(name)
    {
        tool_cfg.overrides.apply_to(&mut policy);
    }

    Ok(policy)
}

/// Determine the base profile from the config chain.
///
/// Returns the profile and the config layer it came from (for applying overrides).
fn resolve_base_profile<'a>(
    tool_name: Option<&str>,
    workspace_config: Option<&'a SandboxConfig>,
    user_config: Option<&'a SandboxConfig>,
) -> (SandboxProfile, Option<&'a SandboxConfig>) {
    // 1. Check tool-level override in workspace config.
    if let Some(name) = tool_name
        && let Some(ws) = workspace_config
        && let Some(tool_cfg) = ws.tools.get(name)
        && let Some(ref profile) = tool_cfg.profile
    {
        return (profile.clone(), Some(ws));
    }

    // 2. Workspace config profile.
    if let Some(ws) = workspace_config {
        return (ws.profile.clone(), Some(ws));
    }

    // 3. User config profile.
    if let Some(user) = user_config {
        return (user.profile.clone(), Some(user));
    }

    // 4. System default.
    (SandboxProfile::default(), None)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::{SandboxOverrides, SandboxPolicyConfig, ToolSandboxConfig};
    use crate::policy::NetMode;

    #[test]
    fn no_config_uses_standard_default() {
        let policy = resolve_policy(None, None, None).unwrap();
        assert_eq!(policy.name, "standard");
    }

    #[test]
    fn workspace_profile_overrides_default() {
        let ws = SandboxConfig {
            profile: SandboxProfile::LockedDown,
            overrides: None,
            profiles: HashMap::new(),
            tools: HashMap::new(),
        };

        let policy = resolve_policy(None, Some(&ws), None).unwrap();
        assert_eq!(policy.name, "locked-down");
    }

    #[test]
    fn user_config_used_when_no_workspace() {
        let user = SandboxConfig {
            profile: SandboxProfile::Unrestricted,
            overrides: None,
            profiles: HashMap::new(),
            tools: HashMap::new(),
        };

        let policy = resolve_policy(None, None, Some(&user)).unwrap();
        assert_eq!(policy.name, "unrestricted");
    }

    #[test]
    fn workspace_overrides_user() {
        let user = SandboxConfig {
            profile: SandboxProfile::Unrestricted,
            overrides: None,
            profiles: HashMap::new(),
            tools: HashMap::new(),
        };
        let ws = SandboxConfig {
            profile: SandboxProfile::LockedDown,
            overrides: None,
            profiles: HashMap::new(),
            tools: HashMap::new(),
        };

        let policy = resolve_policy(None, Some(&ws), Some(&user)).unwrap();
        assert_eq!(policy.name, "locked-down");
    }

    #[test]
    fn tool_override_takes_precedence() {
        let ws = SandboxConfig {
            profile: SandboxProfile::Standard,
            overrides: None,
            profiles: HashMap::new(),
            tools: HashMap::from([(
                "code_runner".to_owned(),
                ToolSandboxConfig {
                    profile: Some(SandboxProfile::LockedDown),
                    overrides: SandboxOverrides::default(),
                },
            )]),
        };

        let policy = resolve_policy(Some("code_runner"), Some(&ws), None).unwrap();
        assert_eq!(policy.name, "locked-down");
    }

    #[test]
    fn tool_not_in_config_falls_back_to_workspace() {
        let ws = SandboxConfig {
            profile: SandboxProfile::Unrestricted,
            overrides: None,
            profiles: HashMap::new(),
            tools: HashMap::new(),
        };

        let policy = resolve_policy(Some("unknown_tool"), Some(&ws), None).unwrap();
        assert_eq!(policy.name, "unrestricted");
    }

    #[test]
    fn workspace_overrides_applied() {
        let ws = SandboxConfig {
            profile: SandboxProfile::Standard,
            overrides: Some(SandboxOverrides {
                fs_write: Some(vec![PathBuf::from("/output")]),
                max_execution_seconds: Some(90),
                ..Default::default()
            }),
            profiles: HashMap::new(),
            tools: HashMap::new(),
        };

        let policy = resolve_policy(None, Some(&ws), None).unwrap();
        assert_eq!(policy.fs_write, vec![PathBuf::from("/output")]);
        assert_eq!(policy.max_execution_seconds, 90);
    }

    #[test]
    fn tool_overrides_applied_on_top() {
        let ws = SandboxConfig {
            profile: SandboxProfile::Standard,
            overrides: Some(SandboxOverrides {
                max_execution_seconds: Some(90),
                ..Default::default()
            }),
            profiles: HashMap::new(),
            tools: HashMap::from([(
                "web_search".to_owned(),
                ToolSandboxConfig {
                    profile: None,
                    overrides: SandboxOverrides {
                        net_allow: Some(vec!["*".into()]),
                        ..Default::default()
                    },
                },
            )]),
        };

        let policy = resolve_policy(Some("web_search"), Some(&ws), None).unwrap();
        // Workspace override applied.
        assert_eq!(policy.max_execution_seconds, 90);
        // Tool override applied on top.
        assert_eq!(policy.net_mode, NetMode::AllowedDomains(vec!["*".into()]));
    }

    #[test]
    fn custom_profile_from_workspace() {
        let ws = SandboxConfig {
            profile: SandboxProfile::Custom("ci-runner".into()),
            overrides: None,
            profiles: HashMap::from([(
                "ci-runner".to_owned(),
                SandboxPolicyConfig {
                    fs_read: vec![PathBuf::from("/workspace")],
                    fs_write: vec![PathBuf::from("/build")],
                    fs_deny: vec![],
                    net_allow: vec!["github.com".into()],
                    process_spawn: true,
                    max_execution_seconds: 120,
                },
            )]),
            tools: HashMap::new(),
        };

        let policy = resolve_policy(None, Some(&ws), None).unwrap();
        assert_eq!(policy.name, "ci-runner");
        assert!(policy.allow_spawn);
    }
}
