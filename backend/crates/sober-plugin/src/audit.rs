//! Synchronous audit pipeline for plugin installation.
//!
//! Before a plugin is created in the registry it must pass an audit pipeline.
//! The pipeline is intentionally synchronous and CPU-only so it can run
//! inside a normal function call without requiring async infrastructure.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sober_core::types::enums::{PluginKind, PluginOrigin};

use sober_core::types::ids::PluginId;

use crate::host_fns::{self, HostContext};
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
impl AuditReport {
    /// Returns `true` if the plugin was approved.
    pub fn is_approved(&self) -> bool {
        self.verdict == AuditVerdict::Approved
    }

    /// Returns the rejection reason, if any.
    pub fn rejection_reason(&self) -> Option<&str> {
        match &self.verdict {
            AuditVerdict::Rejected { reason, .. } => Some(reason),
            AuditVerdict::Approved => None,
        }
    }
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
/// Runs validation stages per plugin kind. WASM plugins additionally go
/// through sandbox loading, capability checking, and embedded test stages.
pub struct AuditPipeline;

impl AuditPipeline {
    /// Runs the audit pipeline for the given request.
    ///
    /// For MCP and Skill plugins a single `validate` stage runs. For WASM
    /// plugins three additional stages execute after validation passes:
    /// `sandbox`, `capability`, and `test`.
    pub fn audit(request: &AuditRequest) -> AuditReport {
        let mut stages = Vec::new();

        let validate_result = match request.kind {
            PluginKind::Mcp => Self::validate_mcp(request),
            PluginKind::Skill => Self::validate_skill(request),
            PluginKind::Wasm => Self::validate_wasm(request),
        };

        stages.push(validate_result);

        // WASM plugins run additional stages when validation passes.
        if request.kind == PluginKind::Wasm && stages.last().is_some_and(|s| s.passed) {
            let sandbox = Self::validate_wasm_sandbox(request);
            stages.push(sandbox);

            if stages.last().is_some_and(|s| s.passed) {
                let capability = Self::validate_wasm_capability(request);
                stages.push(capability);
            }

            if stages.last().is_some_and(|s| s.passed) {
                let test = Self::validate_wasm_test(request);
                stages.push(test);
            }
        }

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

    /// Builds host functions for audit stages from the request's manifest.
    fn audit_host_functions(request: &AuditRequest) -> Vec<extism::Function> {
        let capabilities = request
            .manifest
            .as_ref()
            .map(|m| m.capabilities.to_capabilities())
            .unwrap_or_default();
        let ctx = HostContext::new(PluginId::new(), capabilities);
        host_fns::build_host_functions(ctx)
    }

    /// Tries to load the WASM bytes with Extism to verify the module is valid.
    ///
    /// Host functions are wired from the manifest so that plugins importing
    /// them (e.g. `sober::host_log`) can be instantiated. If `wasm_bytes` is
    /// `None`, the stage passes with a note.
    fn validate_wasm_sandbox(request: &AuditRequest) -> StageResult {
        let Some(ref wasm_bytes) = request.wasm_bytes else {
            return StageResult {
                name: "sandbox".into(),
                passed: true,
                details: "skipped — no WASM bytes provided".into(),
            };
        };

        let wasm = extism::Wasm::data(wasm_bytes.clone());
        let manifest = extism::Manifest::new([wasm]);
        let functions = Self::audit_host_functions(request);

        match extism::Plugin::new(&manifest, functions, true) {
            Ok(_plugin) => StageResult {
                name: "sandbox".into(),
                passed: true,
                details: "WASM module loaded successfully in Extism sandbox".into(),
            },
            Err(e) => StageResult {
                name: "sandbox".into(),
                passed: false,
                details: format!("WASM module failed to load in Extism: {e}"),
            },
        }
    }

    /// Verifies that the manifest capabilities configuration is structurally valid.
    ///
    /// Checks that the declared capabilities in the manifest can be resolved
    /// into the flat capability list the sandbox enforcer uses. This is a
    /// structural validation — runtime permission enforcement happens later.
    fn validate_wasm_capability(request: &AuditRequest) -> StageResult {
        let Some(ref manifest) = request.manifest else {
            // Should not happen — validate_wasm already checked this.
            return StageResult {
                name: "capability".into(),
                passed: false,
                details: "manifest missing (should have been caught by validate stage)".into(),
            };
        };

        let caps = manifest.capabilities.to_capabilities();

        StageResult {
            name: "capability".into(),
            passed: true,
            details: format!(
                "capabilities resolved successfully ({} declared)",
                caps.len()
            ),
        }
    }

    /// Runs the embedded `__sober_test` export if present.
    ///
    /// If the WASM module exports a `__sober_test` function, it is called
    /// with empty input. Success means the call returns without error.
    /// If the function does not exist, the stage passes with a note.
    ///
    /// If `wasm_bytes` is `None`, the stage is skipped.
    fn validate_wasm_test(request: &AuditRequest) -> StageResult {
        let Some(ref wasm_bytes) = request.wasm_bytes else {
            return StageResult {
                name: "test".into(),
                passed: true,
                details: "skipped — no WASM bytes provided".into(),
            };
        };

        let wasm = extism::Wasm::data(wasm_bytes.clone());
        let manifest = extism::Manifest::new([wasm]);
        let functions = Self::audit_host_functions(request);

        let mut plugin = match extism::Plugin::new(&manifest, functions, true) {
            Ok(p) => p,
            Err(e) => {
                return StageResult {
                    name: "test".into(),
                    passed: false,
                    details: format!("cannot load WASM for test stage: {e}"),
                };
            }
        };

        if !plugin.function_exists("__sober_test") {
            return StageResult {
                name: "test".into(),
                passed: true,
                details: "no __sober_test export found — skipped".into(),
            };
        }

        match plugin.call::<&str, &str>("__sober_test", "") {
            Ok(_output) => StageResult {
                name: "test".into(),
                passed: true,
                details: "__sober_test returned success".into(),
            },
            Err(e) => StageResult {
                name: "test".into(),
                passed: false,
                details: format!("__sober_test failed: {e}"),
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

    fn wasm_request_with_bytes(
        manifest: Option<PluginManifest>,
        wasm_bytes: Option<Vec<u8>>,
    ) -> AuditRequest {
        AuditRequest {
            name: "test-wasm".into(),
            kind: PluginKind::Wasm,
            origin: PluginOrigin::Agent,
            config: serde_json::json!({}),
            manifest,
            wasm_bytes,
        }
    }

    fn valid_manifest() -> PluginManifest {
        let toml = r#"
[plugin]
name = "wasm-test"
version = "0.1.0"

[[tools]]
name = "compute"
description = "Computes something"
"#;
        PluginManifest::from_toml(toml).expect("valid manifest")
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
        // validate passes, but sandbox fails because the bytes are not valid WASM
        assert!(matches!(
            report.verdict,
            AuditVerdict::Rejected { ref stage, .. } if stage == "sandbox"
        ));
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

    // -----------------------------------------------------------------------
    // WASM sandbox stage tests
    // -----------------------------------------------------------------------

    #[test]
    fn wasm_sandbox_rejects_invalid_bytes() {
        let manifest = valid_manifest();
        let req = wasm_request_with_bytes(Some(manifest), Some(vec![0xFF, 0xFF, 0xFF]));
        let report = AuditPipeline::audit(&req);

        // validate passes (manifest present), sandbox rejects (invalid WASM).
        assert_eq!(report.stages[0].name, "validate");
        assert!(report.stages[0].passed);
        assert_eq!(report.stages[1].name, "sandbox");
        assert!(!report.stages[1].passed);
        assert!(matches!(
            report.verdict,
            AuditVerdict::Rejected { ref stage, .. } if stage == "sandbox"
        ));
    }

    #[test]
    fn wasm_sandbox_skips_when_no_bytes() {
        let manifest = valid_manifest();
        let req = wasm_request_with_bytes(Some(manifest), None);
        let report = AuditPipeline::audit(&req);

        // validate passes, sandbox skipped (passed with note).
        assert_eq!(report.stages[1].name, "sandbox");
        assert!(report.stages[1].passed);
        assert!(report.stages[1].details.contains("skipped"));
    }

    #[test]
    fn wasm_sandbox_rejects_truncated_wasm_header() {
        let manifest = valid_manifest();
        // Valid WASM magic but incomplete — only the 4-byte magic, no version.
        let req = wasm_request_with_bytes(Some(manifest), Some(b"\0asm".to_vec()));
        let report = AuditPipeline::audit(&req);

        assert_eq!(report.stages[1].name, "sandbox");
        assert!(!report.stages[1].passed);
        assert!(report.stages[1].details.contains("failed to load"));
    }

    // -----------------------------------------------------------------------
    // WASM capability stage tests
    // -----------------------------------------------------------------------

    #[test]
    fn wasm_capability_passes_with_valid_manifest() {
        let manifest = valid_manifest();
        // No wasm_bytes so sandbox is skipped, allowing capability to run.
        let req = wasm_request_with_bytes(Some(manifest), None);
        let report = AuditPipeline::audit(&req);

        let cap_stage = report.stages.iter().find(|s| s.name == "capability");
        assert!(cap_stage.is_some(), "capability stage should have run");
        let cap_stage = cap_stage.expect("checked above");
        assert!(cap_stage.passed);
        assert!(cap_stage.details.contains("resolved successfully"));
    }

    #[test]
    fn wasm_capability_reports_declared_count() {
        let toml = r#"
[plugin]
name = "cap-test"
version = "0.1.0"

[capabilities]
memory_read = true
network = { allowed_hosts = ["api.example.com"] }

[[tools]]
name = "fetch"
description = "Fetches data"
"#;
        let manifest = PluginManifest::from_toml(toml).expect("valid manifest");
        let req = wasm_request_with_bytes(Some(manifest), None);
        let report = AuditPipeline::audit(&req);

        let cap_stage = report
            .stages
            .iter()
            .find(|s| s.name == "capability")
            .expect("capability stage should exist");
        assert!(cap_stage.passed);
        assert!(cap_stage.details.contains("2 declared"));
    }

    #[test]
    fn wasm_capability_not_reached_on_sandbox_failure() {
        let manifest = valid_manifest();
        let req = wasm_request_with_bytes(Some(manifest), Some(vec![0xFF]));
        let report = AuditPipeline::audit(&req);

        // sandbox fails, so capability should not appear.
        assert!(
            !report.stages.iter().any(|s| s.name == "capability"),
            "capability stage should not run after sandbox failure"
        );
    }

    // -----------------------------------------------------------------------
    // WASM test stage tests
    // -----------------------------------------------------------------------

    #[test]
    fn wasm_test_skips_when_no_bytes() {
        let manifest = valid_manifest();
        let req = wasm_request_with_bytes(Some(manifest), None);
        let report = AuditPipeline::audit(&req);

        let test_stage = report
            .stages
            .iter()
            .find(|s| s.name == "test")
            .expect("test stage should exist");
        assert!(test_stage.passed);
        assert!(test_stage.details.contains("skipped"));
    }

    #[test]
    fn wasm_test_not_reached_on_sandbox_failure() {
        let manifest = valid_manifest();
        let req = wasm_request_with_bytes(Some(manifest), Some(vec![0xDE, 0xAD]));
        let report = AuditPipeline::audit(&req);

        assert!(
            !report.stages.iter().any(|s| s.name == "test"),
            "test stage should not run after sandbox failure"
        );
    }

    #[test]
    fn wasm_no_manifest_stops_at_validate() {
        let req = wasm_request_with_bytes(None, Some(vec![0, 97, 115, 109]));
        let report = AuditPipeline::audit(&req);

        assert_eq!(report.stages.len(), 1);
        assert_eq!(report.stages[0].name, "validate");
        assert!(!report.stages[0].passed);
        assert!(matches!(
            report.verdict,
            AuditVerdict::Rejected { ref stage, .. } if stage == "validate"
        ));
    }

    #[test]
    fn wasm_full_pipeline_stages_with_no_bytes() {
        // With a manifest and no bytes, all four stages run (sandbox and test skip gracefully).
        let manifest = valid_manifest();
        let req = wasm_request_with_bytes(Some(manifest), None);
        let report = AuditPipeline::audit(&req);

        assert_eq!(report.stages.len(), 4);
        assert_eq!(report.stages[0].name, "validate");
        assert_eq!(report.stages[1].name, "sandbox");
        assert_eq!(report.stages[2].name, "capability");
        assert_eq!(report.stages[3].name, "test");
        assert!(report.stages.iter().all(|s| s.passed));
        assert_eq!(report.verdict, AuditVerdict::Approved);
    }

    #[test]
    fn mcp_does_not_run_wasm_stages() {
        let req = mcp_request(serde_json::json!({ "command": "node" }));
        let report = AuditPipeline::audit(&req);

        assert_eq!(report.stages.len(), 1);
        assert_eq!(report.stages[0].name, "validate");
    }
}
