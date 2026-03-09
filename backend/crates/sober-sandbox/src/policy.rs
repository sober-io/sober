//! Sandbox policy types and built-in profile definitions.
//!
//! A [`SandboxPolicy`] describes the concrete restrictions applied to a
//! sandboxed process. [`SandboxProfile`] is the user-facing enum that
//! resolves to a policy via [`SandboxProfile::resolve`].

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A named sandbox profile that resolves to a concrete [`SandboxPolicy`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SandboxProfile {
    /// Most restrictive: read-only workspace src, write only `/tmp`, no network.
    LockedDown,
    /// Balanced: read-only workspace, allowed-domains network, no spawning.
    #[default]
    Standard,
    /// Least restrictive: full filesystem and network access.
    Unrestricted,
    /// User-defined profile looked up by name.
    Custom(String),
}

impl SandboxProfile {
    /// Resolve this profile to a concrete [`SandboxPolicy`].
    ///
    /// Built-in profiles return hardcoded defaults. `Custom` profiles are
    /// looked up in the provided registry.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::SandboxError::PolicyResolutionFailed`] if a
    /// custom profile name is not found in the registry.
    pub fn resolve(
        &self,
        custom_profiles: &HashMap<String, SandboxPolicy>,
    ) -> Result<SandboxPolicy, crate::error::SandboxError> {
        match self {
            Self::LockedDown => Ok(SandboxPolicy {
                name: "locked-down".into(),
                fs_read: vec![],
                fs_write: vec![PathBuf::from("/tmp")],
                fs_deny: vec![],
                net_mode: NetMode::None,
                max_execution_seconds: 30,
                allow_spawn: false,
            }),
            Self::Standard => Ok(SandboxPolicy {
                name: "standard".into(),
                fs_read: vec![],
                fs_write: vec![],
                fs_deny: vec![],
                net_mode: NetMode::AllowedDomains(vec![]),
                max_execution_seconds: 60,
                allow_spawn: false,
            }),
            Self::Unrestricted => Ok(SandboxPolicy {
                name: "unrestricted".into(),
                fs_read: vec![],
                fs_write: vec![],
                fs_deny: vec![],
                net_mode: NetMode::Full,
                max_execution_seconds: 300,
                allow_spawn: true,
            }),
            Self::Custom(name) => custom_profiles.get(name).cloned().ok_or_else(|| {
                crate::error::SandboxError::PolicyResolutionFailed(format!(
                    "custom profile '{name}' not found"
                ))
            }),
        }
    }
}

impl Serialize for SandboxProfile {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::LockedDown => serializer.serialize_str("locked-down"),
            Self::Standard => serializer.serialize_str("standard"),
            Self::Unrestricted => serializer.serialize_str("unrestricted"),
            Self::Custom(name) => serializer.serialize_str(name),
        }
    }
}

impl<'de> Deserialize<'de> for SandboxProfile {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "locked-down" => Self::LockedDown,
            "standard" => Self::Standard,
            "unrestricted" => Self::Unrestricted,
            _ => Self::Custom(s),
        })
    }
}

/// Concrete sandbox restrictions applied to a process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPolicy {
    /// Resolved profile name (for audit logging).
    pub name: String,
    /// Paths mounted read-only inside the sandbox.
    pub fs_read: Vec<PathBuf>,
    /// Paths mounted read-write inside the sandbox.
    pub fs_write: Vec<PathBuf>,
    /// Paths masked with `/dev/null` (denied access).
    pub fs_deny: Vec<PathBuf>,
    /// Network access mode.
    pub net_mode: NetMode,
    /// Maximum execution time in seconds before SIGTERM.
    pub max_execution_seconds: u32,
    /// Whether the sandboxed process may spawn child processes.
    pub allow_spawn: bool,
}

/// Network access mode for sandboxed processes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NetMode {
    /// No network access (loopback only via `--unshare-net`).
    None,
    /// Network restricted to specific domains via HTTPS CONNECT proxy.
    AllowedDomains(Vec<String>),
    /// Unrestricted network access (no namespace isolation).
    Full,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_down_defaults() {
        let policy = SandboxProfile::LockedDown.resolve(&HashMap::new()).unwrap();
        assert_eq!(policy.name, "locked-down");
        assert_eq!(policy.net_mode, NetMode::None);
        assert!(!policy.allow_spawn);
        assert_eq!(policy.max_execution_seconds, 30);
        assert_eq!(policy.fs_write, vec![PathBuf::from("/tmp")]);
    }

    #[test]
    fn standard_defaults() {
        let policy = SandboxProfile::Standard.resolve(&HashMap::new()).unwrap();
        assert_eq!(policy.name, "standard");
        assert_eq!(policy.net_mode, NetMode::AllowedDomains(vec![]));
        assert!(!policy.allow_spawn);
        assert_eq!(policy.max_execution_seconds, 60);
    }

    #[test]
    fn unrestricted_defaults() {
        let policy = SandboxProfile::Unrestricted
            .resolve(&HashMap::new())
            .unwrap();
        assert_eq!(policy.name, "unrestricted");
        assert_eq!(policy.net_mode, NetMode::Full);
        assert!(policy.allow_spawn);
        assert_eq!(policy.max_execution_seconds, 300);
    }

    #[test]
    fn custom_profile_lookup() {
        let mut profiles = HashMap::new();
        profiles.insert(
            "ci-runner".to_owned(),
            SandboxPolicy {
                name: "ci-runner".into(),
                fs_read: vec![PathBuf::from("/workspace")],
                fs_write: vec![PathBuf::from("/workspace/build")],
                fs_deny: vec![],
                net_mode: NetMode::AllowedDomains(vec!["github.com".into()]),
                max_execution_seconds: 120,
                allow_spawn: true,
            },
        );

        let policy = SandboxProfile::Custom("ci-runner".into())
            .resolve(&profiles)
            .unwrap();
        assert_eq!(policy.name, "ci-runner");
        assert!(policy.allow_spawn);
    }

    #[test]
    fn custom_profile_not_found() {
        let result = SandboxProfile::Custom("nonexistent".into()).resolve(&HashMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn profile_serde_roundtrip() {
        let profiles = vec![
            (SandboxProfile::LockedDown, "locked-down"),
            (SandboxProfile::Standard, "standard"),
            (SandboxProfile::Unrestricted, "unrestricted"),
            (SandboxProfile::Custom("my-profile".into()), "my-profile"),
        ];

        for (profile, expected_str) in profiles {
            let serialized = serde_json::to_string(&profile).unwrap();
            assert_eq!(serialized, format!("\"{expected_str}\""));

            let deserialized: SandboxProfile = serde_json::from_str(&serialized).unwrap();
            assert_eq!(deserialized, profile);
        }
    }

    #[test]
    fn default_profile_is_standard() {
        assert_eq!(SandboxProfile::default(), SandboxProfile::Standard);
    }
}
