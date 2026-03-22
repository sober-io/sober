//! Template scaffolding for new plugin projects.
//!
//! Generates a plugin skeleton (`plugin.toml`, `Cargo.toml`, `src/lib.rs`, `build.rs`)
//! in a target directory without requiring an LLM.

use std::path::Path;

use crate::GenError;

/// Returns the path to the `sober-pdk` crate.
///
/// Checks `SOBER_PDK_PATH` env var first, falls back to the default
/// container path `/usr/local/share/sober/sdks/sober-pdk`.
pub fn pdk_path() -> String {
    std::env::var("SOBER_PDK_PATH")
        .unwrap_or_else(|_| "/usr/local/share/sober/sdks/sober-pdk".to_string())
}

/// Scaffolds a new plugin project in the given output directory.
///
/// Creates the directory structure with `plugin.toml`, `Cargo.toml`,
/// `src/lib.rs`, and `build.rs`. The `name` parameter is used for the
/// plugin and tool names.
///
/// # Errors
///
/// Returns [`GenError::Io`] if any file or directory creation fails.
pub fn scaffold(name: &str, output_dir: &Path) -> Result<(), GenError> {
    std::fs::create_dir_all(output_dir)?;
    std::fs::create_dir_all(output_dir.join("src"))?;

    std::fs::write(output_dir.join("plugin.toml"), plugin_toml(name))?;
    std::fs::write(output_dir.join("Cargo.toml"), cargo_toml(name, &pdk_path()))?;
    std::fs::write(output_dir.join("src").join("lib.rs"), lib_rs(name))?;
    std::fs::write(output_dir.join("build.rs"), build_rs())?;

    Ok(())
}

fn plugin_toml(name: &str) -> String {
    let fn_name = name.replace('-', "_");
    format!(
        r#"[plugin]
name = "{name}"
version = "0.1.0"
description = "A Sõber plugin"

[capabilities]
# Uncomment capabilities as needed:
# key_value = true
# network = true
# secret_read = true
# tool_call = true
# metrics = true

[[tools]]
name = "{fn_name}"
description = "TODO: describe what this tool does"
"#
    )
}

fn cargo_toml(name: &str, pdk_path: &str) -> String {
    format!(
        r#"[package]
name = "sober-plugin-{name}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
sober-pdk = {{ path = "{pdk_path}" }}
extism-pdk = "1"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#
    )
}

fn lib_rs(name: &str) -> String {
    let fn_name = name.replace('-', "_");
    format!(
        r#"use extism_pdk::*;
use sober_pdk::log;

#[plugin_fn]
pub fn {fn_name}(input: String) -> FnResult<String> {{
    log::info(&format!("{fn_name} called with: {{input}}"));
    Ok(serde_json::json!({{
        "result": "TODO: implement {name}"
    }})
    .to_string())
}}
"#
    )
}

fn build_rs() -> &'static str {
    r#"fn main() {
    // When sober-pdk build_support is available, use:
    // sober_pdk_build_support::emit_capability_flags("plugin.toml");
    println!("cargo:rerun-if-changed=plugin.toml");
}
"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scaffold_to_tempdir(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let output = dir.path().join(name);
        scaffold(name, &output).expect("scaffold failed");
        (dir, output)
    }

    #[test]
    fn creates_all_expected_files() {
        let (_dir, output) = scaffold_to_tempdir("weather");

        assert!(output.join("plugin.toml").exists(), "plugin.toml missing");
        assert!(output.join("Cargo.toml").exists(), "Cargo.toml missing");
        assert!(
            output.join("src").join("lib.rs").exists(),
            "src/lib.rs missing"
        );
        assert!(output.join("build.rs").exists(), "build.rs missing");
    }

    #[test]
    fn plugin_toml_contains_correct_name() {
        let (_dir, output) = scaffold_to_tempdir("weather");

        let contents =
            fs::read_to_string(output.join("plugin.toml")).expect("failed to read plugin.toml");

        assert!(
            contents.contains(r#"name = "weather""#),
            "plugin name not found in plugin.toml"
        );
    }

    #[test]
    fn cargo_toml_is_valid_toml_with_correct_package_name() {
        let (_dir, output) = scaffold_to_tempdir("my_plugin");

        let contents =
            fs::read_to_string(output.join("Cargo.toml")).expect("failed to read Cargo.toml");

        let parsed: toml::Value = toml::from_str(&contents).expect("Cargo.toml is not valid TOML");

        let package_name = parsed
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .expect("package.name missing from Cargo.toml");

        assert_eq!(package_name, "sober-plugin-my_plugin");
    }

    #[test]
    fn src_lib_rs_contains_tool_function() {
        let (_dir, output) = scaffold_to_tempdir("greeter");

        let contents = fs::read_to_string(output.join("src").join("lib.rs"))
            .expect("failed to read src/lib.rs");

        assert!(
            contents.contains("pub fn greeter("),
            "tool function 'greeter' not found in src/lib.rs"
        );
    }

    #[test]
    fn build_rs_contains_rerun_directive() {
        let (_dir, output) = scaffold_to_tempdir("weather");

        let contents =
            fs::read_to_string(output.join("build.rs")).expect("failed to read build.rs");

        assert!(
            contents.contains("cargo:rerun-if-changed=plugin.toml"),
            "rerun directive not found in build.rs"
        );
    }

    #[test]
    fn scaffold_creates_output_dir_if_missing() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let output = dir.path().join("nested").join("plugin_dir");

        scaffold("test_plugin", &output).expect("scaffold failed for nested dir");

        assert!(output.exists(), "output directory was not created");
        assert!(output.join("plugin.toml").exists());
    }

    #[test]
    fn plugin_toml_is_valid_toml() {
        let (_dir, output) = scaffold_to_tempdir("validator");

        let contents =
            fs::read_to_string(output.join("plugin.toml")).expect("failed to read plugin.toml");

        // Strip commented lines before parsing — TOML comments are valid,
        // but the commented-out keys under [capabilities] are just comments.
        toml::from_str::<toml::Value>(&contents).expect("plugin.toml is not valid TOML");
    }
}
