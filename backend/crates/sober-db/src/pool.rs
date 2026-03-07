//! PostgreSQL connection pool creation and configuration.

use sober_core::error::AppError;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

/// Database connection settings.
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// PostgreSQL connection URL.
    pub url: String,
    /// Maximum number of connections in the pool.
    pub max_connections: u32,
}

/// Creates a configured PostgreSQL connection pool.
///
/// Uses consistent settings across all binaries: 5-second acquire timeout,
/// caller-specified max connections (typically 10 for servers, 1 for CLI).
pub async fn create_pool(config: &DatabaseConfig) -> Result<PgPool, AppError> {
    PgPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.url)
        .await
        .map_err(|e| AppError::Internal(e.into()))
}
