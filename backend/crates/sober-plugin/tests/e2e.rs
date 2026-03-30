//! End-to-end tests for the WASM plugin pipeline.
//!
//! Uses WAT (WebAssembly Text format) to define minimal test plugins that
//! exercise the full path: load WASM -> PluginHost -> call tool -> verify output.
//! Also validates the audit pipeline with real WASM modules.

use std::sync::{Arc, Mutex};

use sober_core::types::enums::{PluginKind, PluginOrigin};
use sober_core::types::tool::Tool;
use sober_plugin::audit::{AuditPipeline, AuditRequest, AuditVerdict};
use sober_plugin::host::PluginHost;
use sober_plugin::manifest::{PluginManifest, PluginMeta, ToolEntry};
use sober_plugin::tool::{PluginTool, PluginToolContext, WasmHostState};

// ---------------------------------------------------------------------------
// WAT plugin source — an echo plugin that copies input to output
// ---------------------------------------------------------------------------

/// Minimal Extism-compatible WASM plugin that echoes its input back as output.
///
/// The plugin imports the Extism kernel functions needed to read input and
/// write output, then byte-copies the input buffer to a freshly allocated
/// output buffer.
const ECHO_PLUGIN_WAT: &str = r#"
(module
  ;; Extism kernel imports
  (import "extism:host/env" "input_offset" (func $input_offset (result i64)))
  (import "extism:host/env" "input_length" (func $input_length (result i64)))
  (import "extism:host/env" "alloc"        (func $alloc (param i64) (result i64)))
  (import "extism:host/env" "store_u8"     (func $store_u8 (param i64 i32)))
  (import "extism:host/env" "load_u8"      (func $load_u8 (param i64) (result i32)))
  (import "extism:host/env" "output_set"   (func $output_set (param i64 i64)))

  ;; Echo: reads the full input and writes it back as output
  (func (export "greet") (result i32)
    (local $in_offs i64)
    (local $in_len  i64)
    (local $out_offs i64)
    (local $i i64)

    ;; Grab input location and length
    (local.set $in_offs (call $input_offset))
    (local.set $in_len  (call $input_length))

    ;; Allocate output buffer of the same size
    (local.set $out_offs (call $alloc (local.get $in_len)))

    ;; Copy input to output byte-by-byte
    (local.set $i (i64.const 0))
    (block $break
      (loop $copy
        (br_if $break (i64.ge_u (local.get $i) (local.get $in_len)))
        (call $store_u8
          (i64.add (local.get $out_offs) (local.get $i))
          (call $load_u8 (i64.add (local.get $in_offs) (local.get $i)))
        )
        (local.set $i (i64.add (local.get $i) (i64.const 1)))
        (br $copy)
      )
    )

    ;; Set the output
    (call $output_set (local.get $out_offs) (local.get $in_len))

    ;; Return 0 = success
    i32.const 0
  )

  ;; A second exported function for function_exists testing
  (func (export "other_tool") (result i32)
    i32.const 0
  )
)
"#;

/// Minimal WASM module that only exports a `__sober_test` function.
/// Used to verify the audit pipeline's test stage.
const TEST_EXPORT_WAT: &str = r#"
(module
  (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
  (import "extism:host/env" "output_set" (func $output_set (param i64 i64)))

  ;; greet is needed so the manifest validation passes
  (func (export "greet") (result i32)
    i32.const 0
  )

  ;; The audit pipeline looks for this export
  (func (export "__sober_test") (result i32)
    i32.const 0
  )
)
"#;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compiles WAT source to WASM bytes.
fn wat_to_wasm(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("WAT should compile to valid WASM")
}

/// Creates a manifest declaring a single "greet" tool.
fn greet_manifest() -> PluginManifest {
    PluginManifest {
        plugin: PluginMeta {
            name: "echo-plugin".into(),
            version: "0.1.0".into(),
            description: Some("An echo test plugin".into()),
        },
        capabilities: Default::default(),
        tools: vec![ToolEntry {
            name: "greet".into(),
            description: "Echoes the input back".into(),
        }],
        metrics: vec![],
    }
}

/// Creates a manifest declaring both "greet" and "other_tool".
fn multi_tool_manifest() -> PluginManifest {
    PluginManifest {
        plugin: PluginMeta {
            name: "multi-tool-plugin".into(),
            version: "0.1.0".into(),
            description: Some("A multi-tool test plugin".into()),
        },
        capabilities: Default::default(),
        tools: vec![
            ToolEntry {
                name: "greet".into(),
                description: "Echoes the input back".into(),
            },
            ToolEntry {
                name: "other_tool".into(),
                description: "Does nothing".into(),
            },
        ],
        metrics: vec![],
    }
}

// ===========================================================================
// Test 1: Load WASM and call a tool — verify output matches input (echo)
// ===========================================================================

#[test]
fn load_and_call_tool_echoes_input() {
    let wasm_bytes = wat_to_wasm(ECHO_PLUGIN_WAT);
    let manifest = greet_manifest();

    let mut host = PluginHost::load(&wasm_bytes, &manifest).expect("should load valid WASM plugin");

    let input = serde_json::json!({"name": "world"});
    let output = host
        .call_tool("greet", input.clone())
        .expect("call_tool should succeed");

    // The echo plugin copies the serialized JSON input to output verbatim.
    let expected = serde_json::to_string(&input).expect("serialize input");
    assert_eq!(
        output.content, expected,
        "echo plugin should return the input unchanged"
    );
    assert!(!output.is_error, "output should not be an error");
}

#[test]
fn call_tool_with_empty_input() {
    let wasm_bytes = wat_to_wasm(ECHO_PLUGIN_WAT);
    let manifest = greet_manifest();

    let mut host = PluginHost::load(&wasm_bytes, &manifest).expect("should load valid WASM plugin");

    let input = serde_json::json!({});
    let output = host
        .call_tool("greet", input.clone())
        .expect("call_tool with empty object should succeed");

    let expected = serde_json::to_string(&input).expect("serialize input");
    assert_eq!(output.content, expected);
}

#[test]
fn call_tool_with_string_input() {
    let wasm_bytes = wat_to_wasm(ECHO_PLUGIN_WAT);
    let manifest = greet_manifest();

    let mut host = PluginHost::load(&wasm_bytes, &manifest).expect("should load valid WASM plugin");

    let input = serde_json::json!("hello, sober!");
    let output = host
        .call_tool("greet", input.clone())
        .expect("call_tool with string input should succeed");

    let expected = serde_json::to_string(&input).expect("serialize input");
    assert_eq!(output.content, expected);
}

// ===========================================================================
// Test 2: function_exists checks
// ===========================================================================

#[test]
fn function_exists_for_exported_functions() {
    let wasm_bytes = wat_to_wasm(ECHO_PLUGIN_WAT);
    let manifest = multi_tool_manifest();

    let host = PluginHost::load(&wasm_bytes, &manifest).expect("should load valid WASM plugin");

    assert!(
        host.function_exists("greet"),
        "greet should be exported by the WASM module"
    );
    assert!(
        host.function_exists("other_tool"),
        "other_tool should be exported by the WASM module"
    );
    assert!(
        !host.function_exists("nonexistent"),
        "nonexistent function should not be found"
    );
}

// ===========================================================================
// Test 3: call_tool rejects undeclared tool name
// ===========================================================================

#[test]
fn call_tool_rejects_undeclared_tool() {
    let wasm_bytes = wat_to_wasm(ECHO_PLUGIN_WAT);
    let manifest = greet_manifest(); // Only declares "greet"

    let mut host = PluginHost::load(&wasm_bytes, &manifest).expect("should load valid WASM plugin");

    let result = host.call_tool("other_tool", serde_json::json!({}));
    assert!(result.is_err(), "undeclared tool should be rejected");

    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("tool not found in manifest"),
        "error should mention manifest: {err}"
    );
}

// ===========================================================================
// Test 4: PluginHost metadata accessors
// ===========================================================================

#[test]
fn host_exposes_manifest_and_plugin_id() {
    let wasm_bytes = wat_to_wasm(ECHO_PLUGIN_WAT);
    let manifest = greet_manifest();

    let host = PluginHost::load(&wasm_bytes, &manifest).expect("should load valid WASM plugin");

    assert_eq!(host.manifest().plugin.name, "echo-plugin");
    assert_eq!(host.manifest().plugin.version, "0.1.0");
    assert_eq!(host.manifest().tools.len(), 1);
    assert_eq!(host.manifest().tools[0].name, "greet");

    // Plugin ID should be non-default (v7 UUID).
    let id = host.plugin_id();
    assert_ne!(
        id,
        sober_core::types::ids::PluginId::default(),
        "plugin ID should be unique"
    );
}

// ===========================================================================
// Test 5: PluginTool (Tool trait adapter) integration
// ===========================================================================

#[tokio::test]
async fn plugin_tool_execute_returns_echoed_content() {
    let wasm_bytes = wat_to_wasm(ECHO_PLUGIN_WAT);
    let manifest = greet_manifest();

    let host = PluginHost::load(&wasm_bytes, &manifest).expect("should load valid WASM plugin");
    let host = Arc::new(Mutex::new(host));

    let tool = PluginTool::new(PluginToolContext {
        host_state: WasmHostState::Loaded(Arc::clone(&host)),
        plugin_name: "echo-plugin".into(),
        tool_name: "greet".into(),
        description: "Echoes the input back".into(),
        plugin_id: sober_core::types::ids::PluginId::new(),
        user_id: None,
        workspace_id: None,
        db_pool: None,
    });

    // Verify metadata
    let metadata = tool.metadata();
    assert_eq!(metadata.name, "greet");
    assert_eq!(metadata.description, "Echoes the input back");
    assert!(!metadata.context_modifying);
    assert!(!metadata.internal);

    // Execute through the Tool trait
    let input = serde_json::json!({"greeting": "hello"});
    let output = tool
        .execute(input.clone())
        .await
        .expect("Tool::execute should succeed");

    let expected = serde_json::to_string(&input).expect("serialize input");
    assert_eq!(
        output.content, expected,
        "PluginTool should relay the echoed content"
    );
    assert!(!output.is_error);
}

#[tokio::test]
async fn plugin_tool_multiple_calls_on_shared_host() {
    let wasm_bytes = wat_to_wasm(ECHO_PLUGIN_WAT);
    let manifest = multi_tool_manifest();

    let host = PluginHost::load(&wasm_bytes, &manifest).expect("should load valid WASM plugin");
    let host = Arc::new(Mutex::new(host));

    let greet_tool = PluginTool::new(PluginToolContext {
        host_state: WasmHostState::Loaded(Arc::clone(&host)),
        plugin_name: "echo-plugin".into(),
        tool_name: "greet".into(),
        description: "Echoes the input back".into(),
        plugin_id: sober_core::types::ids::PluginId::new(),
        user_id: None,
        workspace_id: None,
        db_pool: None,
    });

    // Call greet multiple times to verify the host can be reused
    for i in 0..3 {
        let input = serde_json::json!({"call": i});
        let output = greet_tool
            .execute(input.clone())
            .await
            .expect("repeated calls should succeed");

        let expected = serde_json::to_string(&input).expect("serialize input");
        assert_eq!(output.content, expected);
    }
}

// ===========================================================================
// Test 6: Audit pipeline with real WASM module — all stages pass
// ===========================================================================

#[test]
fn audit_pipeline_approves_valid_wasm_module() {
    let wasm_bytes = wat_to_wasm(ECHO_PLUGIN_WAT);
    let manifest = greet_manifest();

    let request = AuditRequest {
        name: "echo-plugin".into(),
        kind: PluginKind::Wasm,
        origin: PluginOrigin::User,
        config: serde_json::json!({}),
        manifest: Some(manifest),
        wasm_bytes: Some(wasm_bytes),
        reserved_tool_names: vec![],
    };

    let report = AuditPipeline::audit(&request);

    // All four WASM stages should run and pass.
    assert_eq!(
        report.stages.len(),
        4,
        "WASM audit should run 4 stages: {:#?}",
        report.stages
    );
    assert_eq!(report.stages[0].name, "validate");
    assert!(report.stages[0].passed, "validate stage should pass");

    assert_eq!(report.stages[1].name, "sandbox");
    assert!(
        report.stages[1].passed,
        "sandbox stage should pass with valid WASM: {}",
        report.stages[1].details
    );

    assert_eq!(report.stages[2].name, "capability");
    assert!(report.stages[2].passed, "capability stage should pass");

    assert_eq!(report.stages[3].name, "test");
    assert!(
        report.stages[3].passed,
        "test stage should pass (no __sober_test export)"
    );

    assert_eq!(
        report.verdict,
        AuditVerdict::Approved,
        "verdict should be Approved"
    );
}

#[test]
fn audit_pipeline_runs_sober_test_export() {
    let wasm_bytes = wat_to_wasm(TEST_EXPORT_WAT);
    let manifest = greet_manifest();

    let request = AuditRequest {
        name: "test-export-plugin".into(),
        kind: PluginKind::Wasm,
        origin: PluginOrigin::Agent,
        config: serde_json::json!({}),
        manifest: Some(manifest),
        wasm_bytes: Some(wasm_bytes),
        reserved_tool_names: vec![],
    };

    let report = AuditPipeline::audit(&request);

    assert_eq!(report.stages.len(), 4);
    assert_eq!(report.verdict, AuditVerdict::Approved);

    // Verify the test stage specifically found and executed __sober_test.
    let test_stage = report
        .stages
        .iter()
        .find(|s| s.name == "test")
        .expect("test stage should exist");
    assert!(test_stage.passed, "test stage should pass");
    assert!(
        test_stage.details.contains("__sober_test returned success"),
        "should report __sober_test success: {}",
        test_stage.details
    );
}

// ===========================================================================
// Test 7: Audit pipeline rejects invalid WASM bytes
// ===========================================================================

#[test]
fn audit_pipeline_rejects_garbage_wasm() {
    let manifest = greet_manifest();

    let request = AuditRequest {
        name: "bad-plugin".into(),
        kind: PluginKind::Wasm,
        origin: PluginOrigin::User,
        config: serde_json::json!({}),
        manifest: Some(manifest),
        wasm_bytes: Some(vec![0xDE, 0xAD, 0xBE, 0xEF]),
        reserved_tool_names: vec![],
    };

    let report = AuditPipeline::audit(&request);

    // validate passes (manifest present), sandbox fails (garbage bytes).
    assert!(report.stages[0].passed, "validate should pass");
    assert!(
        !report.stages[1].passed,
        "sandbox should reject garbage WASM"
    );
    assert!(matches!(
        report.verdict,
        AuditVerdict::Rejected { ref stage, .. } if stage == "sandbox"
    ));
}

// ===========================================================================
// Test 8: Audit pipeline with capabilities
// ===========================================================================

#[test]
fn audit_pipeline_reports_capabilities_from_real_wasm() {
    let wasm_bytes = wat_to_wasm(ECHO_PLUGIN_WAT);
    let manifest_toml = r#"
[plugin]
name = "capable-plugin"
version = "0.1.0"

[capabilities]
key_value = true
memory_read = true

[[tools]]
name = "greet"
description = "Echoes the input back"
"#;
    let manifest = PluginManifest::from_toml(manifest_toml).expect("valid manifest");

    let request = AuditRequest {
        name: "capable-plugin".into(),
        kind: PluginKind::Wasm,
        origin: PluginOrigin::User,
        config: serde_json::json!({}),
        manifest: Some(manifest),
        wasm_bytes: Some(wasm_bytes),
        reserved_tool_names: vec![],
    };

    let report = AuditPipeline::audit(&request);

    assert_eq!(report.verdict, AuditVerdict::Approved);

    let cap_stage = report
        .stages
        .iter()
        .find(|s| s.name == "capability")
        .expect("capability stage should exist");
    assert!(cap_stage.passed);
    assert!(
        cap_stage.details.contains("2 declared"),
        "should report 2 capabilities: {}",
        cap_stage.details
    );
}

// ===========================================================================
// Test 9: Full round-trip — PluginHost loaded with host functions
// ===========================================================================

#[test]
fn host_with_host_functions_can_call_tool() {
    // This test verifies that PluginHost::load (which wires host functions
    // from the manifest's capabilities) still allows the plugin to run.
    let wasm_bytes = wat_to_wasm(ECHO_PLUGIN_WAT);
    let manifest_toml = r#"
[plugin]
name = "full-stack"
version = "0.1.0"

[capabilities]
key_value = true

[[tools]]
name = "greet"
description = "Echoes the input back"
"#;
    let manifest = PluginManifest::from_toml(manifest_toml).expect("valid manifest");

    let mut host =
        PluginHost::load(&wasm_bytes, &manifest).expect("should load with host functions");

    let input = serde_json::json!({"test": "host_functions_wired"});
    let output = host
        .call_tool("greet", input.clone())
        .expect("call_tool should succeed with host functions");

    let expected = serde_json::to_string(&input).expect("serialize");
    assert_eq!(output.content, expected);
}
