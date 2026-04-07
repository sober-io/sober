//! PostgreSQL implementation of [`EvolutionRepo`].

use sober_core::error::AppError;
use sober_core::types::{
    CreateEvolutionEvent, EvolutionConfigRow, EvolutionEvent, EvolutionEventId, EvolutionStatus,
    EvolutionType, UserId,
};
use sqlx::PgPool;

use crate::rows::{EvolutionConfigDbRow, EvolutionEventRow};

/// PostgreSQL-backed evolution event repository.
#[derive(Clone)]
pub struct PgEvolutionRepo {
    pool: PgPool,
}

impl PgEvolutionRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const EVENT_COLUMNS: &str = "id, evolution_type, user_id, title, description, confidence, \
                              source_count, status, payload, result, status_history, \
                              decided_by, reverted_at, created_at, updated_at";

/// Serializes an `AutonomyLevel` to its snake_case string representation for DB storage.
fn autonomy_to_string(level: sober_core::types::AutonomyLevel) -> String {
    // AutonomyLevel uses #[serde(rename_all = "snake_case")], so serializing
    // to JSON gives us a quoted string like "approval_required".
    serde_json::to_value(level)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "approval_required".to_owned())
}

impl sober_core::types::EvolutionRepo for PgEvolutionRepo {
    async fn create(&self, input: CreateEvolutionEvent) -> Result<EvolutionEvent, AppError> {
        let id = EvolutionEventId::new();
        let initial_history = serde_json::json!([{
            "status": serde_json::to_value(input.status)
                .unwrap_or(serde_json::Value::String("proposed".into())),
            "at": chrono::Utc::now().to_rfc3339(),
        }]);

        let row = sqlx::query_as::<_, EvolutionEventRow>(&format!(
            "INSERT INTO evolution_events \
             (id, evolution_type, user_id, title, description, confidence, \
              source_count, status, payload, status_history) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             RETURNING {EVENT_COLUMNS}"
        ))
        .bind(id.as_uuid())
        .bind(input.evolution_type)
        .bind(input.user_id.map(|id| *id.as_uuid()))
        .bind(&input.title)
        .bind(&input.description)
        .bind(input.confidence)
        .bind(input.source_count)
        .bind(input.status)
        .bind(&input.payload)
        .bind(&initial_history)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => {
                AppError::Conflict(
                    "an active evolution with this type and title already exists".into(),
                )
            }
            other => AppError::Internal(other.into()),
        })?;

        Ok(row.into())
    }

    async fn get_by_id(&self, id: EvolutionEventId) -> Result<EvolutionEvent, AppError> {
        let row = sqlx::query_as::<_, EvolutionEventRow>(&format!(
            "SELECT {EVENT_COLUMNS} FROM evolution_events WHERE id = $1"
        ))
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("evolution event".into()))?;

        Ok(row.into())
    }

    async fn list(
        &self,
        r#type: Option<EvolutionType>,
        status: Option<EvolutionStatus>,
    ) -> Result<Vec<EvolutionEvent>, AppError> {
        let rows = sqlx::query_as::<_, EvolutionEventRow>(&format!(
            "SELECT {EVENT_COLUMNS} FROM evolution_events \
             WHERE ($1::evolution_type IS NULL OR evolution_type = $1) \
             AND ($2::evolution_status IS NULL OR status = $2) \
             ORDER BY created_at DESC"
        ))
        .bind(r#type)
        .bind(status)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_active(&self) -> Result<Vec<EvolutionEvent>, AppError> {
        let rows = sqlx::query_as::<_, EvolutionEventRow>(&format!(
            "SELECT {EVENT_COLUMNS} FROM evolution_events \
             WHERE status = 'active' \
             ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn update_status(
        &self,
        id: EvolutionEventId,
        status: EvolutionStatus,
        decided_by: Option<UserId>,
    ) -> Result<(), AppError> {
        let new_entry = serde_json::json!([{
            "status": serde_json::to_value(status)
                .unwrap_or(serde_json::Value::String("proposed".into())),
            "at": chrono::Utc::now().to_rfc3339(),
        }]);

        let result = sqlx::query(
            "UPDATE evolution_events \
             SET status = $1, \
                 updated_at = now(), \
                 status_history = status_history || $2::jsonb, \
                 decided_by = COALESCE($3, decided_by), \
                 reverted_at = CASE WHEN $1 = 'reverted' THEN now() ELSE reverted_at END \
             WHERE id = $4",
        )
        .bind(status)
        .bind(&new_entry)
        .bind(decided_by.map(|id| *id.as_uuid()))
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("evolution event".into()));
        }

        Ok(())
    }

    async fn update_result(
        &self,
        id: EvolutionEventId,
        result: serde_json::Value,
    ) -> Result<(), AppError> {
        let affected = sqlx::query(
            "UPDATE evolution_events SET result = $1, updated_at = now() WHERE id = $2",
        )
        .bind(&result)
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if affected.rows_affected() == 0 {
            return Err(AppError::NotFound("evolution event".into()));
        }

        Ok(())
    }

    async fn delete(&self, id: EvolutionEventId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM evolution_events WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("evolution event".into()));
        }

        Ok(())
    }

    async fn count_auto_approved_today(&self) -> Result<i64, AppError> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM evolution_events \
             WHERE decided_by IS NULL \
             AND status IN ('approved', 'executing', 'active') \
             AND created_at >= current_date",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(count)
    }

    async fn count_executing(&self) -> Result<i64, AppError> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM evolution_events WHERE status = 'executing'")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;

        Ok(count)
    }

    async fn list_timeline(
        &self,
        limit: i64,
        r#type: Option<EvolutionType>,
        status: Option<EvolutionStatus>,
    ) -> Result<Vec<EvolutionEvent>, AppError> {
        let rows = sqlx::query_as::<_, EvolutionEventRow>(&format!(
            "SELECT {EVENT_COLUMNS} FROM evolution_events \
             WHERE ($1::evolution_type IS NULL OR evolution_type = $1) \
             AND ($2::evolution_status IS NULL OR status = $2) \
             ORDER BY created_at DESC \
             LIMIT $3"
        ))
        .bind(r#type)
        .bind(status)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_config(&self) -> Result<EvolutionConfigRow, AppError> {
        let row = sqlx::query_as::<_, EvolutionConfigDbRow>(
            "SELECT id, plugin_autonomy, skill_autonomy, instruction_autonomy, \
             automation_autonomy, updated_at \
             FROM evolution_config WHERE id = TRUE",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("evolution config".into()))?;

        Ok(row.into())
    }

    async fn update_config(
        &self,
        config: EvolutionConfigRow,
    ) -> Result<EvolutionConfigRow, AppError> {
        let row = sqlx::query_as::<_, EvolutionConfigDbRow>(
            "UPDATE evolution_config \
             SET plugin_autonomy = $1, \
                 skill_autonomy = $2, \
                 instruction_autonomy = $3, \
                 automation_autonomy = $4, \
                 updated_at = now() \
             WHERE id = TRUE \
             RETURNING id, plugin_autonomy, skill_autonomy, instruction_autonomy, \
                       automation_autonomy, updated_at",
        )
        .bind(autonomy_to_string(config.plugin_autonomy))
        .bind(autonomy_to_string(config.skill_autonomy))
        .bind(autonomy_to_string(config.instruction_autonomy))
        .bind(autonomy_to_string(config.automation_autonomy))
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("Evolution config not found".into()))?;

        Ok(row.into())
    }
}
