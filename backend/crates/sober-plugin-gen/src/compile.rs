//! WASM compilation for plugin source directories.
//!
//! Shells out to `cargo build --target wasm32-wasip1 --release` and returns the
//! compiled `.wasm` bytes.

use std::path::Path;

use tokio::{fs, process::Command};

use crate::GenError;

/// Compiles a Rust plugin project to WASM.
///
/// Runs `cargo build --target wasm32-wasip1 --release` in `source_dir`,
/// then reads and returns the compiled `.wasm` file bytes.
///
/// The WASM output is located at:
/// `source_dir/target/wasm32-wasip1/release/<crate_name>.wasm`
///
/// The crate name is read from `source_dir/Cargo.toml`. Hyphens in crate
/// names are converted to underscores in the output filename, matching
/// Cargo's naming convention.
pub async fn compile(source_dir: &Path) -> Result<Vec<u8>, GenError> {
    let crate_name = read_crate_name(source_dir).await?;

    let output = Command::new("cargo")
        .args(["build", "--target", "wasm32-wasip1", "--release"])
        .current_dir(source_dir)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(GenError::Compile(stderr));
    }

    // Cargo converts hyphens to underscores in the output binary name.
    let wasm_name = crate_name.replace('-', "_");
    let wasm_path = source_dir
        .join("target")
        .join("wasm32-wasip1")
        .join("release")
        .join(format!("{wasm_name}.wasm"));

    let bytes = fs::read(&wasm_path).await?;
    Ok(bytes)
}

/// Reads the `[package].name` field from `source_dir/Cargo.toml`.
async fn read_crate_name(source_dir: &Path) -> Result<String, GenError> {
    let cargo_toml_path = source_dir.join("Cargo.toml");
    let contents = fs::read_to_string(&cargo_toml_path).await?;
    let manifest: toml::Value = toml::from_str(&contents)?;

    let name = manifest
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| GenError::Compile("Cargo.toml is missing [package].name".to_string()))?
        .to_string();

    Ok(name)
}

#[cfg(test)]
mod tests {
    use std::io;

    use tempfile::TempDir;
    use tokio::fs;

    use super::*;

    /// Returns `GenError::Io` when `source_dir` does not exist.
    #[tokio::test]
    async fn compile_nonexistent_dir_returns_io_error() {
        let result = compile(Path::new("/tmp/nonexistent-sober-plugin-gen-test-dir")).await;
        assert!(
            matches!(result, Err(GenError::Io(ref e)) if e.kind() == io::ErrorKind::NotFound),
            "expected Io(NotFound), got {result:?}"
        );
    }

    /// Returns `GenError::Compile` when source_dir exists but has no valid Cargo project.
    #[tokio::test]
    async fn compile_invalid_project_returns_compile_error() {
        let dir = TempDir::new().expect("create temp dir");

        // Write a syntactically valid Cargo.toml but with no src/ — cargo will fail.
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "test-plugin"
version = "0.1.0"
edition = "2021"
"#,
        )
        .await
        .expect("write Cargo.toml");

        let result = compile(dir.path()).await;
        assert!(
            matches!(result, Err(GenError::Compile(_))),
            "expected Compile error, got {result:?}"
        );
    }

    /// Returns `GenError::Compile` when Cargo.toml is missing [package].name.
    #[tokio::test]
    async fn compile_missing_package_name_returns_compile_error() {
        let dir = TempDir::new().expect("create temp dir");

        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[dependencies]
tokio = "1"
"#,
        )
        .await
        .expect("write Cargo.toml");

        let result = compile(dir.path()).await;
        assert!(
            matches!(result, Err(GenError::Compile(_))),
            "expected Compile error for missing package name, got {result:?}"
        );
    }

    /// Returns `GenError::Toml` when Cargo.toml is malformed TOML.
    #[tokio::test]
    async fn compile_malformed_toml_returns_toml_error() {
        let dir = TempDir::new().expect("create temp dir");

        fs::write(dir.path().join("Cargo.toml"), "this is [[[not valid toml")
            .await
            .expect("write Cargo.toml");

        let result = compile(dir.path()).await;
        assert!(
            matches!(result, Err(GenError::Toml(_))),
            "expected Toml error, got {result:?}"
        );
    }
}
