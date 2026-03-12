//! PostgreSQL implementation of [`JobRepo`] and [`JobRunRepo`].

use chrono::{DateTime, Utc};
use sober_core::error::AppError;
use sober_core::types::{CreateJob, Job, JobId, JobRun, JobRunId, JobStatus};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::{JobRow, JobRunRow};

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

const JOB_COLUMNS: &str = "id, name, schedule, status, payload, payload_bytes, \
                            owner_type, owner_id, notify_agent, next_run_at, \
                            last_run_at, created_at";

impl sober_core::types::JobRepo for PgJobRepo {
    async fn create(&self, input: CreateJob) -> Result<Job, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, JobRow>(&format!(
            "INSERT INTO jobs (id, name, schedule, status, payload, payload_bytes, \
             owner_type, owner_id, notify_agent, next_run_at) \
             VALUES ($1, $2, $3, 'active', $4, $5, $6, $7, $8, $9) \
             RETURNING {JOB_COLUMNS}"
        ))
        .bind(id)
        .bind(&input.name)
        .bind(&input.schedule)
        .bind(&input.payload)
        .bind(&input.payload_bytes)
        .bind(&input.owner_type)
        .bind(input.owner_id)
        .bind(input.notify_agent)
        .bind(input.next_run_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn get_by_id(&self, id: JobId) -> Result<Job, AppError> {
        let row =
            sqlx::query_as::<_, JobRow>(&format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id = $1"))
                .bind(id.as_uuid())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?
                .ok_or_else(|| AppError::NotFound("job".into()))?;

        Ok(row.into())
    }

    async fn list_active(&self) -> Result<Vec<Job>, AppError> {
        let rows = sqlx::query_as::<_, JobRow>(&format!(
            "SELECT {JOB_COLUMNS} FROM jobs WHERE status = 'active' \
             ORDER BY next_run_at ASC NULLS LAST"
        ))
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
        let result = sqlx::query("UPDATE jobs SET status = $1 WHERE id = $2 AND status != $1")
            .bind(JobStatus::Cancelled)
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("job".into()));
        }

        Ok(())
    }

    async fn update_status(&self, id: JobId, status: JobStatus) -> Result<(), AppError> {
        let result = sqlx::query("UPDATE jobs SET status = $1 WHERE id = $2")
            .bind(status)
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("job".into()));
        }

        Ok(())
    }

    async fn list_due(&self, now: DateTime<Utc>) -> Result<Vec<Job>, AppError> {
        let rows = sqlx::query_as::<_, JobRow>(&format!(
            "SELECT {JOB_COLUMNS} FROM jobs \
             WHERE status = 'active' AND next_run_at <= $1 \
             ORDER BY next_run_at ASC"
        ))
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_filtered(
        &self,
        owner_type: Option<&str>,
        owner_id: Option<uuid::Uuid>,
        status: Option<&str>,
    ) -> Result<Vec<Job>, AppError> {
        // Build dynamic WHERE clause
        let mut conditions = Vec::new();
        if owner_type.is_some() {
            conditions.push("owner_type = $1");
        }
        if owner_id.is_some() {
            conditions.push("owner_id = $2");
        }
        if status.is_some() {
            conditions.push("status = $3");
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };

        let query =
            format!("SELECT {JOB_COLUMNS} FROM jobs{where_clause} ORDER BY created_at DESC");

        let mut q = sqlx::query_as::<_, JobRow>(&query);
        if let Some(ot) = owner_type {
            q = q.bind(ot.to_owned());
        }
        if let Some(oi) = owner_id {
            q = q.bind(oi);
        }
        if let Some(s) = status {
            q = q.bind(s.to_owned());
        }

        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }
}

/// PostgreSQL-backed job run repository.
pub struct PgJobRunRepo {
    pool: PgPool,
}

impl PgJobRunRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::JobRunRepo for PgJobRunRepo {
    async fn create(&self, job_id: JobId) -> Result<JobRun, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, JobRunRow>(
            "INSERT INTO job_runs (id, job_id) VALUES ($1, $2) \
             RETURNING id, job_id, started_at, finished_at, status, result, error",
        )
        .bind(id)
        .bind(job_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn complete(
        &self,
        id: JobRunId,
        result: Vec<u8>,
        error: Option<String>,
    ) -> Result<(), AppError> {
        let status = if error.is_some() {
            "failed"
        } else {
            "succeeded"
        };
        let affected = sqlx::query(
            "UPDATE job_runs SET finished_at = now(), status = $1, result = $2, error = $3 \
             WHERE id = $4",
        )
        .bind(status)
        .bind(&result)
        .bind(&error)
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if affected.rows_affected() == 0 {
            return Err(AppError::NotFound("job_run".into()));
        }

        Ok(())
    }

    async fn list_by_job(&self, job_id: JobId, limit: u32) -> Result<Vec<JobRun>, AppError> {
        let rows = sqlx::query_as::<_, JobRunRow>(
            "SELECT id, job_id, started_at, finished_at, status, result, error \
             FROM job_runs WHERE job_id = $1 \
             ORDER BY started_at DESC LIMIT $2",
        )
        .bind(job_id.as_uuid())
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }
}
