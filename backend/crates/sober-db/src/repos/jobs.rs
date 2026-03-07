//! PostgreSQL implementation of [`JobRepo`].

use chrono::{DateTime, Utc};
use sober_core::error::AppError;
use sober_core::types::{CreateJob, Job, JobId};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::JobRow;

/// PostgreSQL-backed job repository.
pub struct PgJobRepo {
    pool: PgPool,
}

impl PgJobRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::JobRepo for PgJobRepo {
    async fn create(&self, input: CreateJob) -> Result<Job, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, JobRow>(
            "INSERT INTO jobs (id, name, schedule, status, payload, next_run_at) \
             VALUES ($1, $2, $3, 'active', $4, $5) \
             RETURNING id, name, schedule, status, payload, next_run_at, last_run_at, created_at",
        )
        .bind(id)
        .bind(&input.name)
        .bind(&input.schedule)
        .bind(&input.payload)
        .bind(input.next_run_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn get_by_id(&self, id: JobId) -> Result<Job, AppError> {
        let row = sqlx::query_as::<_, JobRow>(
            "SELECT id, name, schedule, status, payload, next_run_at, last_run_at, created_at \
             FROM jobs WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("job".into()))?;

        Ok(row.into())
    }

    async fn list_active(&self) -> Result<Vec<Job>, AppError> {
        let rows = sqlx::query_as::<_, JobRow>(
            "SELECT id, name, schedule, status, payload, next_run_at, last_run_at, created_at \
             FROM jobs WHERE status = 'active' \
             ORDER BY next_run_at ASC NULLS LAST",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn update_next_run(&self, id: JobId, next_run_at: DateTime<Utc>) -> Result<(), AppError> {
        let result = sqlx::query("UPDATE jobs SET next_run_at = $1 WHERE id = $2")
            .bind(next_run_at)
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("job".into()));
        }

        Ok(())
    }

    async fn mark_last_run(&self, id: JobId, ran_at: DateTime<Utc>) -> Result<(), AppError> {
        let result = sqlx::query("UPDATE jobs SET last_run_at = $1 WHERE id = $2")
            .bind(ran_at)
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("job".into()));
        }

        Ok(())
    }

    async fn cancel(&self, id: JobId) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE jobs SET status = 'cancelled' WHERE id = $1 AND status != 'cancelled'",
        )
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("job".into()));
        }

        Ok(())
    }
}
