//! Plugin generation pipeline: template scaffolding, WASM compilation, and LLM-powered generation.
//!
//! # Planned modules
//!
//! - `scaffold` — Template scaffolding (no LLM); generates plugin skeleton files from a manifest.
//! - `compile` — WASM compilation; shells out to `cargo build --target wasm32-wasip1`.
//! - `generate` — LLM-powered generation with a self-correcting retry loop.

pub mod error;

pub use error::GenError;
