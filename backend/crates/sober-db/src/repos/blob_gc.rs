//! PostgreSQL implementation of [`BlobGcRepo`].

use sober_core::error::AppError;
use sqlx::PgPool;

/// PostgreSQL-backed blob GC repository.
#[derive(Clone)]
pub struct PgBlobGcRepo {
    pool: PgPool,
}

impl PgBlobGcRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::repo::BlobGcRepo for PgBlobGcRepo {
    async fn find_unreferenced(&self, keys: &[String]) -> Result<Vec<String>, AppError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT key FROM unnest($1::text[]) AS key \
             WHERE NOT EXISTS (SELECT 1 FROM conversation_attachments WHERE blob_key = key) \
               AND NOT EXISTS (SELECT 1 FROM plugins WHERE config->>'wasm_blob_key' = key \
                                                        OR config->>'manifest_blob_key' = key) \
               AND NOT EXISTS (SELECT 1 FROM artifacts WHERE blob_key = key AND state != 'archived')",
        )
        .bind(keys)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(|(k,)| k).collect())
    }
}
