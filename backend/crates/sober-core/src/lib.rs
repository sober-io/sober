//! Shared types, error handling, configuration, and domain primitives for the
//! Sober system.
//!
//! `sober-core` is the foundation crate — every other crate in the workspace
//! depends on it. It has no dependencies on any sibling crate.

pub mod admin;
pub mod config;
pub mod error;
pub mod telemetry;
pub mod types;

#[cfg(feature = "test-utils")]
pub mod test_utils;

// Re-exports for ergonomic imports
pub use admin::{AdminCommand, AdminResponse};
pub use config::AppConfig;
pub use error::{ApiErrorBody, AppError};
pub use telemetry::{MetricsEndpoint, TelemetryGuard, init_telemetry};
pub use types::*;

// Re-export commonly used external types
pub use chrono::{DateTime, Utc};
pub use uuid::Uuid;
