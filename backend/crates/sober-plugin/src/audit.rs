//! Synchronous audit pipeline for plugin installation.
//!
//! Before a plugin is created in the registry it must pass an audit pipeline.
//! The pipeline is intentionally synchronous and CPU-only so it can run
//! inside a normal function call without requiring async infrastructure.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sober_core::types::enums::{PluginKind, PluginOrigin};

use crate::manifest::PluginManifest;

/// Input to the audit pipeline.
#[derive(Debug, Clone)]
pub struct AuditRequest {
    /// Plugin name.
    pub name: String,
    /// Plugin kind (MCP, Skill, WASM).
    pub kind: PluginKind,
    /// How the plugin was discovered.
    pub origin: PluginOrigin,
    /// Plugin-specific configuration (JSON).
    pub config: serde_json::Value,
    /// Parsed manifest (required for WASM plugins).
    pub manifest: Option<PluginManifest>,
    /// Raw WASM bytes (for future static analysis).
    pub wasm_bytes: Option<Vec<u8>>,
}

/// Result of a single audit stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    /// Stage name (e.g. `"validate"`).
    pub name: String,
    /// Whether the stage passed.
    pub passed: bool,
    /// Human-readable details or failure reason.
    pub details: String,
}

/// Overall verdict of an audit run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "lowercase")]
pub enum AuditVerdict {
    /// The plugin passed all audit stages.
    Approved,
    /// The plugin was rejected.
    Rejected {
        /// Which stage rejected the plugin.
        stage: String,
        /// Why it was rejected.
        reason: String,
    },
}

/// Full audit report produced by the pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    /// Plugin name.
    pub plugin_name: String,
    /// Plugin kind.
    pub plugin_kind: PluginKind,
    /// Plugin origin.
    pub origin: PluginOrigin,
    /// Results of each pipeline stage.
    pub stages: Vec<StageResult>,
    /// Overall verdict.
    pub verdict: AuditVerdict,
    /// When the audit was performed.
    pub timestamp: DateTime<Utc>,
}

/// The audit pipeline.
///
/// Currently runs a single `validate` stage per plugin kind. More stages
/// (static analysis, behavioral analysis, etc.) will be added as the
/// sandbox infrastructure matures.
pub struct AuditPipeline;

impl AuditPipeline {
    /// Runs the audit pipeline for the given request.
    pub fn audit(request: &AuditRequest) -> AuditReport {
        let mut stages = Vec::new();

        let validate_result = match request.kind {
            PluginKind::Mcp => Self::validate_mcp(request),
            PluginKind::Skill => Self::validate_skill(request),
            PluginKind::Wasm => Self::validate_wasm(request),
        };

        stages.push(validate_result);

        let verdict = stages
            .iter()
            .find(|s| !s.passed)
            .map(|s| AuditVerdict::Rejected {
                stage: s.name.clone(),
                reason: s.details.clone(),
            })
            .unwrap_or(AuditVerdict::Approved);

        AuditReport {
            plugin_name: request.name.clone(),
            plugin_kind: request.kind,
            origin: request.origin,
            stages,
            verdict,
            timestamp: Utc::now(),
        }
    }

    /// Validates an MCP plugin: config must contain a `"command"` string.
    fn validate_mcp(request: &AuditRequest) -> StageResult {
        let passed = request
            .config
            .get("command")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());

        StageResult {
            name: "validate".into(),
            passed,
            details: if passed {
                "MCP config has valid command".into()
            } else {
                "MCP config must contain a non-empty \"command\" string".into()
            },
        }
    }

    /// Validates a Skill plugin: config must contain a `"path"` string.
    fn validate_skill(request: &AuditRequest) -> StageResult {
        let passed = request
            .config
            .get("path")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());

        StageResult {
            name: "validate".into(),
            passed,
            details: if passed {
                "Skill config has valid path".into()
            } else {
                "Skill config must contain a non-empty \"path\" string".into()
            },
        }
    }

    /// Validates a WASM plugin: manifest must be present.
    fn validate_wasm(request: &AuditRequest) -> StageResult {
        let passed = request.manifest.is_some();

        StageResult {
            name: "validate".into(),
            passed,
            details: if passed {
                "WASM plugin has manifest".into()
            } else {
                "WASM plugin must include a manifest".into()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mcp_request(config: serde_json::Value) -> AuditRequest {
        AuditRequest {
            name: "test-mcp".into(),
            kind: PluginKind::Mcp,
            origin: PluginOrigin::User,
            config,
            manifest: None,
            wasm_bytes: None,
        }
    }

    fn skill_request(config: serde_json::Value) -> AuditRequest {
        AuditRequest {
            name: "test-skill".into(),
            kind: PluginKind::Skill,
            origin: PluginOrigin::User,
            config,
            manifest: None,
            wasm_bytes: None,
        }
    }

    fn wasm_request(manifest: Option<PluginManifest>) -> AuditRequest {
        AuditRequest {
            name: "test-wasm".into(),
            kind: PluginKind::Wasm,
            origin: PluginOrigin::Agent,
            config: serde_json::json!({}),
            manifest,
            wasm_bytes: Some(vec![0, 97, 115, 109]),
        }
    }

    #[test]
    fn mcp_valid_config() {
        let req = mcp_request(serde_json::json!({ "command": "npx", "args": ["-y", "server"] }));
        let report = AuditPipeline::audit(&req);
        assert_eq!(report.verdict, AuditVerdict::Approved);
        assert!(report.stages[0].passed);
    }

    #[test]
    fn mcp_missing_command() {
        let req = mcp_request(serde_json::json!({ "args": ["--port", "3000"] }));
        let report = AuditPipeline::audit(&req);
        assert!(matches!(
            report.verdict,
            AuditVerdict::Rejected { stage, .. } if stage == "validate"
        ));
        assert!(!report.stages[0].passed);
    }

    #[test]
    fn mcp_empty_command() {
        let req = mcp_request(serde_json::json!({ "command": "" }));
        let report = AuditPipeline::audit(&req);
        assert!(matches!(report.verdict, AuditVerdict::Rejected { .. }));
    }

    #[test]
    fn skill_valid_config() {
        let req = skill_request(serde_json::json!({ "path": "/usr/local/bin/skill" }));
        let report = AuditPipeline::audit(&req);
        assert_eq!(report.verdict, AuditVerdict::Approved);
    }

    #[test]
    fn skill_missing_path() {
        let req = skill_request(serde_json::json!({ "name": "no-path" }));
        let report = AuditPipeline::audit(&req);
        assert!(matches!(report.verdict, AuditVerdict::Rejected { .. }));
    }

    #[test]
    fn wasm_with_manifest() {
        let manifest_toml = r#"
[plugin]
name = "wasm-test"
version = "0.1.0"

[[tools]]
name = "compute"
description = "Computes something"
"#;
        let manifest = PluginManifest::from_toml(manifest_toml).expect("valid manifest");
        let req = wasm_request(Some(manifest));
        let report = AuditPipeline::audit(&req);
        assert_eq!(report.verdict, AuditVerdict::Approved);
    }

    #[test]
    fn wasm_without_manifest() {
        let req = wasm_request(None);
        let report = AuditPipeline::audit(&req);
        assert!(matches!(report.verdict, AuditVerdict::Rejected { .. }));
    }

    #[test]
    fn report_has_timestamp_and_metadata() {
        let req = mcp_request(serde_json::json!({ "command": "node" }));
        let report = AuditPipeline::audit(&req);
        assert_eq!(report.plugin_name, "test-mcp");
        assert_eq!(report.plugin_kind, PluginKind::Mcp);
        assert_eq!(report.origin, PluginOrigin::User);
        assert!(!report.stages.is_empty());
    }
}
