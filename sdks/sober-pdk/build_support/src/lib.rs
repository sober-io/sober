//! # Sober PDK Build Support
//!
//! Build-time helper for Sober plugin authors. Reads a `plugin.toml` manifest
//! and emits `cargo:rustc-cfg` flags for each declared capability, so the
//! corresponding PDK modules become available at compile time.
//!
//! # Usage
//!
//! In a plugin's `build.rs`:
//!
//! ```rust,ignore
//! fn main() {
//!     sober_pdk_build::emit_capability_flags("plugin.toml");
//! }
//! ```

use std::fs;

/// All capability keys recognized in `plugin.toml`.
const CAPABILITY_KEYS: &[&str] = &[
    "memory_read",
    "memory_write",
    "network",
    "filesystem",
    "llm_call",
    "tool_call",
    "conversation_read",
    "metrics",
    "secret_read",
    "key_value",
    "schedule",
];

/// Reads a `plugin.toml` manifest and emits `cargo:rustc-cfg` flags
/// for each enabled capability.
///
/// Call this from a plugin's `build.rs`:
///
/// ```rust,ignore
/// fn main() {
///     sober_pdk_build::emit_capability_flags("plugin.toml");
/// }
/// ```
///
/// A capability is considered enabled if its value is `true` or an inline
/// table (any non-`false` structured value). Unrecognized keys are silently
/// ignored.
///
/// Also emits `cargo:rerun-if-changed={manifest_path}` so Cargo re-runs
/// the build script when the manifest changes.
///
/// # Panics
///
/// Panics if the manifest file cannot be read or parsed. Build scripts
/// are expected to fail loudly on configuration errors.
pub fn emit_capability_flags(manifest_path: &str) {
    println!("cargo:rerun-if-changed={manifest_path}");

    let content = fs::read_to_string(manifest_path)
        .unwrap_or_else(|e| panic!("failed to read {manifest_path}: {e}"));

    let flags = resolve_capability_flags(&content);

    for flag in &flags {
        println!("cargo:rustc-cfg=feature=\"{flag}\"");
    }
}

/// Parses TOML content and returns the list of capability feature flags
/// that should be enabled.
///
/// This is the pure logic behind [`emit_capability_flags`], exposed for
/// testing without filesystem or stdout side effects.
pub fn resolve_capability_flags(toml_content: &str) -> Vec<String> {
    let toml: toml::Value = toml_content
        .parse()
        .unwrap_or_else(|e| panic!("failed to parse plugin.toml: {e}"));

    let Some(caps) = toml.get("capabilities").and_then(|v| v.as_table()) else {
        return Vec::new();
    };

    let mut flags = Vec::new();

    for key in CAPABILITY_KEYS {
        if let Some(value) = caps.get(*key) {
            let enabled = match value {
                toml::Value::Boolean(b) => *b,
                toml::Value::Table(_) => true, // inline table = enabled with config
                _ => false,
            };
            if enabled {
                flags.push((*key).to_string());
            }
        }
    }

    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_capabilities_enabled() {
        let toml = r#"
[plugin]
name = "test-plugin"

[capabilities]
memory_read = true
memory_write = true
network = true
filesystem = true
llm_call = true
tool_call = true
conversation_read = true
metrics = true
secret_read = true
key_value = true
schedule = true
"#;
        let flags = resolve_capability_flags(toml);
        assert_eq!(
            flags,
            vec![
                "memory_read",
                "memory_write",
                "network",
                "filesystem",
                "llm_call",
                "tool_call",
                "conversation_read",
                "metrics",
                "secret_read",
                "key_value",
                "schedule",
            ]
        );
    }

    #[test]
    fn mixed_boolean_and_inline_table() {
        let toml = r#"
[capabilities]
network = true
filesystem = false
key_value = { max_keys = 100 }
metrics = true
"#;
        let flags = resolve_capability_flags(toml);
        assert_eq!(flags, vec!["network", "metrics", "key_value"]);
    }

    #[test]
    fn some_disabled() {
        let toml = r#"
[capabilities]
memory_read = true
memory_write = false
network = false
secret_read = true
"#;
        let flags = resolve_capability_flags(toml);
        assert_eq!(flags, vec!["memory_read", "secret_read"]);
    }

    #[test]
    fn no_capabilities_section() {
        let toml = r#"
[plugin]
name = "bare-plugin"
version = "0.1.0"
"#;
        let flags = resolve_capability_flags(toml);
        assert!(flags.is_empty());
    }

    #[test]
    fn empty_capabilities_section() {
        let toml = r#"
[capabilities]
"#;
        let flags = resolve_capability_flags(toml);
        assert!(flags.is_empty());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let toml = r#"
[capabilities]
network = true
quantum_teleport = true
time_travel = true
metrics = true
"#;
        let flags = resolve_capability_flags(toml);
        assert_eq!(flags, vec!["network", "metrics"]);
    }
}
