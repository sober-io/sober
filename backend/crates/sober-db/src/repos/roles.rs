//! PostgreSQL implementation of [`RoleRepo`].

use sober_core::error::AppError;
use sober_core::types::{ScopeId, UserId};
use sqlx::PgPool;

/// PostgreSQL-backed role repository.
pub struct PgRoleRepo {
    pool: PgPool,
}

impl PgRoleRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::RoleRepo for PgRoleRepo {
    async fn get_roles_for_user(&self, user_id: UserId) -> Result<Vec<String>, AppError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT r.name FROM roles r \
             JOIN user_roles ur ON ur.role_id = r.id \
             WHERE ur.user_id = $1 AND ur.scope_id = $2",
        )
        .bind(user_id.as_uuid())
        .bind(ScopeId::GLOBAL.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(|(name,)| name).collect())
    }
}
