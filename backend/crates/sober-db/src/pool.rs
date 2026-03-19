//! PostgreSQL connection pool creation and configuration.

use metrics::gauge;
use sober_core::error::AppError;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

/// Interval between pool metric snapshots.
const POOL_METRICS_INTERVAL: Duration = Duration::from_secs(15);

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
///
/// Spawns a background task that periodically records pool connection metrics
/// (`sober_pg_pool_connections_active`, `sober_pg_pool_connections_idle`).
/// Per-service filtering in dashboards uses the Prometheus `job` label
/// added automatically by the scrape config.
pub async fn create_pool(config: &DatabaseConfig) -> Result<PgPool, AppError> {
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.url)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let pool_for_metrics = pool.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(POOL_METRICS_INTERVAL);
        loop {
            interval.tick().await;
            let active = pool_for_metrics.size() as f64;
            let idle = pool_for_metrics.num_idle() as f64;
            gauge!("sober_pg_pool_connections_active").set(active);
            gauge!("sober_pg_pool_connections_idle").set(idle);
        }
    });

    Ok(pool)
}
