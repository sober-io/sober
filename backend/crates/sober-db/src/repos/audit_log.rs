//! PostgreSQL implementation of [`AuditLogRepo`].

use sober_core::error::AppError;
use sober_core::types::{AuditLogEntry, CreateAuditLog, UserId};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::AuditLogRow;

/// PostgreSQL-backed audit log repository.
#[derive(Clone)]
pub struct PgAuditLogRepo {
    pool: PgPool,
}

impl PgAuditLogRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::AuditLogRepo for PgAuditLogRepo {
    async fn create(&self, input: CreateAuditLog) -> Result<AuditLogEntry, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, AuditLogRow>(
            "INSERT INTO audit_log (id, actor_id, action, target_type, target_id, details, ip_address) \
             VALUES ($1, $2, $3, $4, $5, $6, $7::inet) \
             RETURNING id, actor_id, action, target_type, target_id, details, \
                       host(ip_address)::text AS ip_address, created_at",
        )
        .bind(id)
        .bind(input.actor_id.map(|id| *id.as_uuid()))
        .bind(&input.action)
        .bind(&input.target_type)
        .bind(input.target_id)
        .bind(&input.details)
        .bind(&input.ip_address)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn list_recent(&self, limit: i64) -> Result<Vec<AuditLogEntry>, AppError> {
        let rows = sqlx::query_as::<_, AuditLogRow>(
            "SELECT id, actor_id, action, target_type, target_id, details, \
                    host(ip_address)::text AS ip_address, created_at \
             FROM audit_log \
             ORDER BY created_at DESC \
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_by_actor(
        &self,
        actor_id: UserId,
        limit: i64,
    ) -> Result<Vec<AuditLogEntry>, AppError> {
        let rows = sqlx::query_as::<_, AuditLogRow>(
            "SELECT id, actor_id, action, target_type, target_id, details, \
                    host(ip_address)::text AS ip_address, created_at \
             FROM audit_log \
             WHERE actor_id = $1 \
             ORDER BY created_at DESC \
             LIMIT $2",
        )
        .bind(actor_id.as_uuid())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }
}
