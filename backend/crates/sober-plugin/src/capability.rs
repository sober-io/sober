//! Plugin capability declarations and configuration.
//!
//! Every plugin declares which host capabilities it needs via a
//! [`CapabilitiesConfig`] section in its manifest.  At install time these
//! are validated against the audit policy and resolved into a flat
//! [`Vec<Capability>`] that the sandbox enforcer understands.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Capability enum — the flat, resolved form the sandbox enforcer uses
// ---------------------------------------------------------------------------

/// A single capability that a plugin may request.
///
/// Variants that carry restriction data provide the sandbox enforcer with
/// the exact limits (allowed scopes, domains, paths, tools).  Variants
/// without config are simple permission flags.
///
/// Serialized with a `kind` tag so it can appear in JSON audit logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Capability {
    /// Read from the memory/context system.
    MemoryRead { scopes: Vec<String> },
    /// Write to the memory/context system.
    MemoryWrite { scopes: Vec<String> },
    /// Make outbound network requests.
    Network { domains: Vec<String> },
    /// Access the filesystem.
    Filesystem { paths: Vec<String> },
    /// Invoke an LLM provider.
    LlmCall,
    /// Call other tools/plugins.
    ToolCall { tools: Vec<String> },
    /// Read conversation history.
    ConversationRead,
    /// Expose or collect metrics.
    Metrics,
    /// Read secrets from the vault.
    SecretRead,
    /// Use plugin-scoped key-value storage.
    KeyValue,
    /// Create or manage scheduled jobs.
    Schedule,
}

// ---------------------------------------------------------------------------
// Cap<T> — enabled/disabled toggle, optionally carrying extra config
// ---------------------------------------------------------------------------

/// A capability toggle that is either a boolean or a config object.
///
/// In the manifest TOML a capability can appear as:
///
/// ```toml
/// memory_read = true
/// network = { allowed_hosts = ["api.example.com"] }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Cap<T> {
    /// Simple on/off toggle.
    Enabled(bool),
    /// Enabled with additional configuration.
    WithConfig(T),
}

impl<T> Cap<T> {
    /// Returns `true` when this capability is active.
    ///
    /// `WithConfig` is always considered enabled.
    pub fn is_enabled(&self) -> bool {
        match self {
            Cap::Enabled(v) => *v,
            Cap::WithConfig(_) => true,
        }
    }
}

impl<T> Default for Cap<T> {
    fn default() -> Self {
        Cap::Enabled(false)
    }
}

// ---------------------------------------------------------------------------
// Per-capability config structs
// ---------------------------------------------------------------------------

/// Network capability configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkCap {
    /// List of allowed host patterns (e.g. `["api.example.com"]`).
    pub allowed_hosts: Vec<String>,
}

/// Memory capability configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryCap {
    /// Which scopes the plugin may access.
    pub scopes: Vec<String>,
}

/// Filesystem capability configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesystemCap {
    /// Allowed path prefixes.
    pub allowed_paths: Vec<String>,
    /// Whether write access is permitted.
    #[serde(default)]
    pub writable: bool,
}

/// Tool-call capability configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallCap {
    /// Tool names the plugin is allowed to invoke.
    pub allowed_tools: Vec<String>,
}

// ---------------------------------------------------------------------------
// CapabilitiesConfig — the manifest-level declaration
// ---------------------------------------------------------------------------

/// Full capability declaration as it appears in a plugin manifest.
///
/// Each field defaults to `Cap::Enabled(false)` when absent.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CapabilitiesConfig {
    /// Read from memory/context.
    #[serde(default)]
    pub memory_read: Cap<MemoryCap>,
    /// Write to memory/context.
    #[serde(default)]
    pub memory_write: Cap<MemoryCap>,
    /// Outbound network access.
    #[serde(default)]
    pub network: Cap<NetworkCap>,
    /// Filesystem access.
    #[serde(default)]
    pub filesystem: Cap<FilesystemCap>,
    /// Invoke an LLM provider.
    #[serde(default)]
    pub llm_call: Cap<bool>,
    /// Call other tools/plugins.
    #[serde(default)]
    pub tool_call: Cap<ToolCallCap>,
    /// Read conversation history.
    #[serde(default)]
    pub conversation_read: Cap<bool>,
    /// Expose or collect metrics.
    #[serde(default)]
    pub metrics: Cap<bool>,
    /// Read secrets from the vault.
    #[serde(default)]
    pub secret_read: Cap<bool>,
    /// Plugin-scoped key-value storage.
    #[serde(default)]
    pub key_value: Cap<bool>,
    /// Create or manage scheduled jobs.
    #[serde(default)]
    pub schedule: Cap<bool>,
}

impl CapabilitiesConfig {
    /// Resolves the config into a flat list of enabled capabilities.
    ///
    /// When a capability has a `WithConfig` variant the restriction data is
    /// propagated into the corresponding [`Capability`] variant.  Boolean-only
    /// toggles produce variants with empty restriction vecs.
    pub fn to_capabilities(&self) -> Vec<Capability> {
        let mut caps = Vec::new();

        if let Cap::WithConfig(ref c) = self.memory_read {
            caps.push(Capability::MemoryRead {
                scopes: c.scopes.clone(),
            });
        } else if self.memory_read.is_enabled() {
            caps.push(Capability::MemoryRead { scopes: vec![] });
        }

        if let Cap::WithConfig(ref c) = self.memory_write {
            caps.push(Capability::MemoryWrite {
                scopes: c.scopes.clone(),
            });
        } else if self.memory_write.is_enabled() {
            caps.push(Capability::MemoryWrite { scopes: vec![] });
        }

        if let Cap::WithConfig(ref c) = self.network {
            caps.push(Capability::Network {
                domains: c.allowed_hosts.clone(),
            });
        } else if self.network.is_enabled() {
            caps.push(Capability::Network { domains: vec![] });
        }

        if let Cap::WithConfig(ref c) = self.filesystem {
            caps.push(Capability::Filesystem {
                paths: c.allowed_paths.clone(),
            });
        } else if self.filesystem.is_enabled() {
            caps.push(Capability::Filesystem { paths: vec![] });
        }

        if self.llm_call.is_enabled() {
            caps.push(Capability::LlmCall);
        }

        if let Cap::WithConfig(ref c) = self.tool_call {
            caps.push(Capability::ToolCall {
                tools: c.allowed_tools.clone(),
            });
        } else if self.tool_call.is_enabled() {
            caps.push(Capability::ToolCall { tools: vec![] });
        }

        if self.conversation_read.is_enabled() {
            caps.push(Capability::ConversationRead);
        }
        if self.metrics.is_enabled() {
            caps.push(Capability::Metrics);
        }
        if self.secret_read.is_enabled() {
            caps.push(Capability::SecretRead);
        }
        if self.key_value.is_enabled() {
            caps.push(Capability::KeyValue);
        }
        if self.schedule.is_enabled() {
            caps.push(Capability::Schedule);
        }
        caps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_enabled_bool() {
        let cap: Cap<bool> = Cap::Enabled(true);
        assert!(cap.is_enabled());

        let cap: Cap<bool> = Cap::Enabled(false);
        assert!(!cap.is_enabled());
    }

    #[test]
    fn cap_with_config_is_enabled() {
        let cap = Cap::WithConfig(NetworkCap {
            allowed_hosts: vec!["example.com".into()],
        });
        assert!(cap.is_enabled());
    }

    #[test]
    fn cap_default_is_disabled() {
        let cap: Cap<bool> = Cap::default();
        assert!(!cap.is_enabled());
    }

    #[test]
    fn capabilities_config_to_capabilities() {
        let config = CapabilitiesConfig {
            memory_read: Cap::Enabled(true),
            network: Cap::WithConfig(NetworkCap {
                allowed_hosts: vec!["api.example.com".into()],
            }),
            metrics: Cap::Enabled(true),
            ..Default::default()
        };

        let caps = config.to_capabilities();
        assert_eq!(caps.len(), 3);
        assert!(caps.contains(&Capability::MemoryRead { scopes: vec![] }));
        assert!(caps.contains(&Capability::Network {
            domains: vec!["api.example.com".into()]
        }));
        assert!(caps.contains(&Capability::Metrics));
    }

    #[test]
    fn memory_write_with_scopes() {
        let config = CapabilitiesConfig {
            memory_write: Cap::WithConfig(MemoryCap {
                scopes: vec!["user".into()],
            }),
            ..Default::default()
        };

        let caps = config.to_capabilities();
        assert_eq!(caps.len(), 1);
        assert!(caps.contains(&Capability::MemoryWrite {
            scopes: vec!["user".into()]
        }));
    }

    #[test]
    fn memory_write_bool_true_gives_empty_scopes() {
        let config = CapabilitiesConfig {
            memory_write: Cap::Enabled(true),
            ..Default::default()
        };

        let caps = config.to_capabilities();
        assert_eq!(caps.len(), 1);
        assert!(caps.contains(&Capability::MemoryWrite { scopes: vec![] }));
    }

    #[test]
    fn capabilities_config_defaults_empty() {
        let config = CapabilitiesConfig::default();
        assert!(config.to_capabilities().is_empty());
    }

    #[test]
    fn cap_serde_roundtrip_bool() {
        let json = serde_json::json!(true);
        let cap: Cap<bool> = serde_json::from_value(json).expect("deserialize bool");
        assert!(cap.is_enabled());
    }

    #[test]
    fn cap_serde_roundtrip_config() {
        let json = serde_json::json!({ "allowed_hosts": ["example.com"] });
        let cap: Cap<NetworkCap> = serde_json::from_value(json).expect("deserialize config");
        assert!(cap.is_enabled());
        if let Cap::WithConfig(cfg) = &cap {
            assert_eq!(cfg.allowed_hosts, vec!["example.com".to_string()]);
        }
    }
}
