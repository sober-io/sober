//! PostgreSQL implementation of [`McpServerRepo`].

use sober_core::error::AppError;
use sober_core::types::{CreateMcpServer, McpServerConfig, McpServerId, UpdateMcpServer, UserId};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::McpServerRow;

/// PostgreSQL-backed MCP server configuration repository.
pub struct PgMcpServerRepo {
    pool: PgPool,
}

impl PgMcpServerRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::McpServerRepo for PgMcpServerRepo {
    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<McpServerConfig>, AppError> {
        let rows = sqlx::query_as::<_, McpServerRow>(
            "SELECT id, user_id, name, command, args, env, enabled, created_at, updated_at \
             FROM mcp_servers WHERE user_id = $1 \
             ORDER BY name ASC",
        )
        .bind(user_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn create(&self, input: CreateMcpServer) -> Result<McpServerConfig, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, McpServerRow>(
            "INSERT INTO mcp_servers (id, user_id, name, command, args, env) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             RETURNING id, user_id, name, command, args, env, enabled, created_at, updated_at",
        )
        .bind(id)
        .bind(input.user_id.as_uuid())
        .bind(&input.name)
        .bind(&input.command)
        .bind(&input.args)
        .bind(&input.env)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => {
                AppError::Conflict("MCP server with this name already exists".into())
            }
            other => AppError::Internal(other.into()),
        })?;

        Ok(row.into())
    }

    async fn update(
        &self,
        id: McpServerId,
        input: UpdateMcpServer,
    ) -> Result<McpServerConfig, AppError> {
        let row = sqlx::query_as::<_, McpServerRow>(
            "UPDATE mcp_servers SET \
                name = COALESCE($1, name), \
                command = COALESCE($2, command), \
                args = COALESCE($3, args), \
                env = COALESCE($4, env), \
                enabled = COALESCE($5, enabled), \
                updated_at = now() \
             WHERE id = $6 \
             RETURNING id, user_id, name, command, args, env, enabled, created_at, updated_at",
        )
        .bind(input.name)
        .bind(input.command)
        .bind(input.args)
        .bind(input.env)
        .bind(input.enabled)
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("mcp_server".into()))?;

        Ok(row.into())
    }

    async fn delete(&self, id: McpServerId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM mcp_servers WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("mcp_server".into()));
        }

        Ok(())
    }
}
