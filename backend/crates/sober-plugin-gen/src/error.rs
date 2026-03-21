//! Error types for the plugin generation pipeline.

use thiserror::Error;

/// Errors that can occur during plugin generation, scaffolding, or compilation.
#[derive(Debug, Error)]
pub enum GenError {
    /// File system error during scaffold or compile operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to parse a TOML plugin manifest.
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    /// WASM compilation failed. Contains the compiler stderr output.
    #[error("WASM compilation failed: {0}")]
    Compile(String),

    /// LLM-powered generation failed after exhausting all retries.
    #[error("LLM generation failed: {0}")]
    Generate(String),

    /// Template rendering error.
    #[error("Template error: {0}")]
    Template(String),
}
