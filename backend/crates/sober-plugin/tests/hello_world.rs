//! Smoke test for the sample hello-world plugin.
//!
//! Loads the pre-compiled WASM from `~/.sober/plugins/hello-world/` and
//! exercises the full pipeline: audit → load → call tools → verify output.
//!
//! Ignored by default — run with:
//!
//! ```sh
//! cargo test -p sober-plugin --test hello_world -- --ignored
//! ```

use sober_core::types::enums::{PluginKind, PluginOrigin};
use sober_plugin::audit::{AuditPipeline, AuditRequest, AuditVerdict};
use sober_plugin::host::PluginHost;
use sober_plugin::manifest::PluginManifest;

fn wasm_path() -> std::path::PathBuf {
    dirs::home_dir()
        .expect("home dir")
        .join(".sober/plugins/hello-world/target/wasm32-wasip1/release/hello_world.wasm")
}

fn manifest() -> PluginManifest {
    let toml = std::fs::read_to_string(
        dirs::home_dir()
            .expect("home dir")
            .join(".sober/plugins/hello-world/plugin.toml"),
    )
    .expect("read plugin.toml");
    PluginManifest::from_toml(&toml).expect("parse manifest")
}

#[test]
#[ignore = "requires pre-compiled hello-world plugin at ~/.sober/plugins/"]
fn audit_approves_hello_world() {
    let wasm_bytes = std::fs::read(wasm_path()).expect("read WASM");
    let manifest = manifest();

    let report = AuditPipeline::audit(&AuditRequest {
        name: "hello-world".into(),
        kind: PluginKind::Wasm,
        origin: PluginOrigin::User,
        config: serde_json::json!({}),
        manifest: Some(manifest),
        wasm_bytes: Some(wasm_bytes),
    });

    assert_eq!(report.verdict, AuditVerdict::Approved);
    for stage in &report.stages {
        eprintln!("  stage {:12} — {}", stage.name, stage.details);
        assert!(
            stage.passed,
            "stage {} failed: {}",
            stage.name, stage.details
        );
    }
}

#[test]
#[ignore = "requires pre-compiled hello-world plugin at ~/.sober/plugins/"]
fn greet_returns_estonian_greeting() {
    let wasm_bytes = std::fs::read(wasm_path()).expect("read WASM");
    let manifest = manifest();
    let mut host = PluginHost::load(&wasm_bytes, &manifest).expect("load plugin");

    let output = host
        .call_tool(
            "greet",
            serde_json::json!({"name": "Harri", "style": "estonian"}),
        )
        .expect("call greet");

    eprintln!("output: {}", output.content);
    let parsed: serde_json::Value = serde_json::from_str(&output.content).expect("parse JSON");
    assert!(parsed["message"].as_str().unwrap().contains("Tere"));
    assert_eq!(parsed["total_greetings"], 1);
}

#[test]
#[ignore = "requires pre-compiled hello-world plugin at ~/.sober/plugins/"]
fn greeting_count_tracks_across_calls() {
    let wasm_bytes = std::fs::read(wasm_path()).expect("read WASM");
    let manifest = manifest();
    let mut host = PluginHost::load(&wasm_bytes, &manifest).expect("load plugin");

    // Greet twice
    host.call_tool("greet", serde_json::json!({"name": "Sõber"}))
        .expect("greet 1");
    host.call_tool("greet", serde_json::json!({"name": "Sõber"}))
        .expect("greet 2");

    // Check count
    let output = host
        .call_tool("greeting_count", serde_json::json!({"name": "Sõber"}))
        .expect("call count");

    let parsed: serde_json::Value = serde_json::from_str(&output.content).expect("parse JSON");
    assert_eq!(parsed["count"], 2);
    assert_eq!(parsed["name"], "Sõber");
}
