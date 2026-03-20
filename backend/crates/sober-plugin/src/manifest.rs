//! Plugin manifest parsing and validation.
//!
//! Every plugin ships a `plugin.toml` manifest that declares its metadata,
//! required capabilities, exposed tools, and metric declarations.

use serde::{Deserialize, Serialize};

use crate::capability::CapabilitiesConfig;
use crate::error::PluginError;

/// Top-level plugin manifest (parsed from TOML).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Core metadata.
    pub plugin: PluginMeta,
    /// Capability declarations.
    #[serde(default)]
    pub capabilities: CapabilitiesConfig,
    /// Tools this plugin exposes.
    #[serde(default)]
    pub tools: Vec<ToolEntry>,
    /// Metric declarations (required when the `metrics` capability is enabled).
    #[serde(default)]
    pub metrics: Vec<MetricDeclaration>,
}

/// Core plugin metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    /// Human-readable plugin name.
    pub name: String,
    /// Semantic version string.
    pub version: String,
    /// Short description.
    #[serde(default)]
    pub description: Option<String>,
}

/// A tool exposed by the plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEntry {
    /// Tool name (must be unique within the plugin).
    pub name: String,
    /// Human-readable description.
    pub description: String,
}

/// A metric the plugin intends to emit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDeclaration {
    /// Metric name (Prometheus-style).
    pub name: String,
    /// Metric kind (`counter`, `gauge`, `histogram`).
    pub kind: String,
    /// Human-readable description.
    pub description: String,
}

impl PluginManifest {
    /// Parses and validates a manifest from TOML content.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::ManifestInvalid`] when parsing fails or
    /// validation rules are violated.
    pub fn from_toml(content: &str) -> Result<Self, PluginError> {
        let manifest: Self =
            toml::from_str(content).map_err(|e| PluginError::ManifestInvalid(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Runs validation rules on a parsed manifest.
    fn validate(&self) -> Result<(), PluginError> {
        if self.plugin.name.trim().is_empty() {
            return Err(PluginError::ManifestInvalid(
                "plugin name must not be empty".into(),
            ));
        }

        if self.tools.is_empty() {
            return Err(PluginError::ManifestInvalid(
                "plugin must declare at least one tool".into(),
            ));
        }

        // If the metrics capability is enabled, at least one metric must be declared.
        if self.capabilities.metrics.is_enabled() && self.metrics.is_empty() {
            return Err(PluginError::ManifestInvalid(
                "metrics capability enabled but no metrics declared".into(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_MANIFEST: &str = r#"
[plugin]
name = "test-plugin"
version = "0.1.0"
description = "A test plugin"

[capabilities]
memory_read = true
network = { allowed_hosts = ["api.example.com"] }

[[tools]]
name = "do_thing"
description = "Does a thing"
"#;

    const MINIMAL_MANIFEST: &str = r#"
[plugin]
name = "minimal"
version = "1.0.0"

[[tools]]
name = "ping"
description = "Pings"
"#;

    #[test]
    fn parse_valid_manifest() {
        let manifest =
            PluginManifest::from_toml(VALID_MANIFEST).expect("should parse valid manifest");
        assert_eq!(manifest.plugin.name, "test-plugin");
        assert_eq!(manifest.plugin.version, "0.1.0");
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools[0].name, "do_thing");

        let caps = manifest.capabilities.to_capabilities();
        assert!(caps.contains(&crate::Capability::MemoryRead { scopes: vec![] }));
        assert!(caps.contains(&crate::Capability::Network {
            domains: vec!["api.example.com".into()],
        }));
    }

    #[test]
    fn parse_minimal_manifest() {
        let manifest =
            PluginManifest::from_toml(MINIMAL_MANIFEST).expect("should parse minimal manifest");
        assert_eq!(manifest.plugin.name, "minimal");
        assert!(manifest.capabilities.to_capabilities().is_empty());
    }

    #[test]
    fn reject_missing_name() {
        let toml = r#"
[plugin]
name = ""
version = "1.0.0"

[[tools]]
name = "t"
description = "d"
"#;
        let err = PluginManifest::from_toml(toml).unwrap_err();
        assert!(err.to_string().contains("plugin name must not be empty"));
    }

    #[test]
    fn reject_empty_tools() {
        let toml = r#"
[plugin]
name = "no-tools"
version = "1.0.0"
"#;
        let err = PluginManifest::from_toml(toml).unwrap_err();
        assert!(err.to_string().contains("at least one tool"));
    }

    #[test]
    fn reject_metrics_cap_without_declarations() {
        let toml = r#"
[plugin]
name = "metrics-missing"
version = "1.0.0"

[capabilities]
metrics = true

[[tools]]
name = "t"
description = "d"
"#;
        let err = PluginManifest::from_toml(toml).unwrap_err();
        assert!(err.to_string().contains("no metrics declared"));
    }

    #[test]
    fn metrics_cap_with_declarations_ok() {
        let toml = r#"
[plugin]
name = "metrics-ok"
version = "1.0.0"

[capabilities]
metrics = true

[[tools]]
name = "t"
description = "d"

[[metrics]]
name = "my_counter"
kind = "counter"
description = "Counts things"
"#;
        let manifest =
            PluginManifest::from_toml(toml).expect("should accept metrics with declarations");
        assert_eq!(manifest.metrics.len(), 1);
    }
}
