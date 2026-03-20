//! PostgreSQL implementation of [`PluginRepo`].

use sober_core::error::AppError;
use sober_core::types::{
    CreatePlugin, CreatePluginAuditLog, Plugin, PluginAuditLog, PluginFilter, PluginId,
    PluginStatus,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::{PluginAuditLogRow, PluginRow};

/// PostgreSQL-backed plugin registry repository.
pub struct PgPluginRepo {
    pool: PgPool,
}

impl PgPluginRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const PLUGIN_COLUMNS: &str = "id, name, kind, version, description, origin, scope, \
                               owner_id, workspace_id, status, config, installed_by, \
                               installed_at, updated_at";

const AUDIT_LOG_COLUMNS: &str = "id, plugin_id, plugin_name, kind, origin, stages, \
                                  verdict, rejection_reason, audited_at, audited_by";

impl sober_core::types::PluginRepo for PgPluginRepo {
    async fn create(&self, input: CreatePlugin) -> Result<Plugin, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, PluginRow>(&format!(
            "INSERT INTO plugins (id, name, kind, version, description, origin, scope, \
             owner_id, workspace_id, status, config, installed_by) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
             RETURNING {PLUGIN_COLUMNS}"
        ))
        .bind(id)
        .bind(&input.name)
        .bind(input.kind)
        .bind(&input.version)
        .bind(&input.description)
        .bind(input.origin)
        .bind(input.scope)
        .bind(input.owner_id.map(|id| *id.as_uuid()))
        .bind(input.workspace_id.map(|id| *id.as_uuid()))
        .bind(input.status)
        .bind(&input.config)
        .bind(input.installed_by.map(|id| *id.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => {
                AppError::Conflict("plugin with this name already exists in scope".into())
            }
            other => AppError::Internal(other.into()),
        })?;

        Ok(row.into())
    }

    async fn get_by_id(&self, id: PluginId) -> Result<Plugin, AppError> {
        let row = sqlx::query_as::<_, PluginRow>(&format!(
            "SELECT {PLUGIN_COLUMNS} FROM plugins WHERE id = $1"
        ))
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("plugin".into()))?;

        Ok(row.into())
    }

    async fn get_by_name(&self, name: &str) -> Result<Plugin, AppError> {
        let row = sqlx::query_as::<_, PluginRow>(&format!(
            "SELECT {PLUGIN_COLUMNS} FROM plugins WHERE name = $1"
        ))
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("plugin".into()))?;

        Ok(row.into())
    }

    async fn list(&self, filter: PluginFilter) -> Result<Vec<Plugin>, AppError> {
        let rows = sqlx::query_as::<_, PluginRow>(&format!(
            "SELECT {PLUGIN_COLUMNS} FROM plugins \
             WHERE ($1::plugin_kind IS NULL OR kind = $1) \
             AND ($2::plugin_scope IS NULL OR scope = $2) \
             AND ($3::uuid IS NULL OR owner_id = $3) \
             AND ($4::uuid IS NULL OR workspace_id = $4) \
             AND ($5::plugin_status IS NULL OR status = $5) \
             AND ($6::text IS NULL OR name = $6) \
             ORDER BY name ASC"
        ))
        .bind(filter.kind)
        .bind(filter.scope)
        .bind(filter.owner_id.map(|id| *id.as_uuid()))
        .bind(filter.workspace_id.map(|id| *id.as_uuid()))
        .bind(filter.status)
        .bind(filter.name)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn update_status(&self, id: PluginId, status: PluginStatus) -> Result<(), AppError> {
        let result =
            sqlx::query("UPDATE plugins SET status = $1, updated_at = now() WHERE id = $2")
                .bind(status)
                .bind(id.as_uuid())
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("plugin".into()));
        }

        Ok(())
    }

    async fn update_config(&self, id: PluginId, config: serde_json::Value) -> Result<(), AppError> {
        let result =
            sqlx::query("UPDATE plugins SET config = $1, updated_at = now() WHERE id = $2")
                .bind(&config)
                .bind(id.as_uuid())
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("plugin".into()));
        }

        Ok(())
    }

    async fn delete(&self, id: PluginId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM plugins WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("plugin".into()));
        }

        Ok(())
    }

    async fn create_audit_log(
        &self,
        input: CreatePluginAuditLog,
    ) -> Result<PluginAuditLog, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, PluginAuditLogRow>(&format!(
            "INSERT INTO plugin_audit_logs (id, plugin_id, plugin_name, kind, origin, \
             stages, verdict, rejection_reason, audited_by) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
             RETURNING {AUDIT_LOG_COLUMNS}"
        ))
        .bind(id)
        .bind(input.plugin_id.map(|id| *id.as_uuid()))
        .bind(&input.plugin_name)
        .bind(input.kind)
        .bind(input.origin)
        .bind(&input.stages)
        .bind(&input.verdict)
        .bind(&input.rejection_reason)
        .bind(input.audited_by.map(|id| *id.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn list_audit_logs(
        &self,
        plugin_id: PluginId,
        limit: i64,
    ) -> Result<Vec<PluginAuditLog>, AppError> {
        let rows = sqlx::query_as::<_, PluginAuditLogRow>(&format!(
            "SELECT {AUDIT_LOG_COLUMNS} FROM plugin_audit_logs \
             WHERE plugin_id = $1 \
             ORDER BY audited_at DESC \
             LIMIT $2"
        ))
        .bind(plugin_id.as_uuid())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_kv_data(
        &self,
        plugin_id: PluginId,
        key: &str,
    ) -> Result<Option<serde_json::Value>, AppError> {
        let row: Option<(serde_json::Value,)> =
            sqlx::query_as("SELECT data->$2 FROM plugin_kv_data WHERE plugin_id = $1")
                .bind(plugin_id.as_uuid())
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.and_then(|(v,)| if v.is_null() { None } else { Some(v) }))
    }

    async fn set_kv_data(
        &self,
        plugin_id: PluginId,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO plugin_kv_data (plugin_id, data, updated_at) \
             VALUES ($1, jsonb_build_object($2::text, $3), now()) \
             ON CONFLICT (plugin_id) DO UPDATE \
             SET data = plugin_kv_data.data || jsonb_build_object($2::text, $3), \
                 updated_at = now()",
        )
        .bind(plugin_id.as_uuid())
        .bind(key)
        .bind(&value)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }
}
