//! PostgreSQL implementation of [`UserRepo`].

use sober_core::error::AppError;
use sober_core::types::{CreateUser, User, UserId, UserStatus};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::UserRow;

/// PostgreSQL-backed user repository.
pub struct PgUserRepo {
    pool: PgPool,
}

impl PgUserRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::UserRepo for PgUserRepo {
    async fn get_by_id(&self, id: UserId) -> Result<User, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, username, password_hash, status, created_at, updated_at \
             FROM users WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("user".into()))?;

        Ok(row.into())
    }

    async fn get_by_email(&self, email: &str) -> Result<User, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, username, password_hash, status, created_at, updated_at \
             FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("user".into()))?;

        Ok(row.into())
    }

    async fn get_by_username(&self, username: &str) -> Result<User, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, username, password_hash, status, created_at, updated_at \
             FROM users WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("user".into()))?;

        Ok(row.into())
    }

    async fn create(&self, input: CreateUser) -> Result<User, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, UserRow>(
            "INSERT INTO users (id, email, username, password_hash, status) \
             VALUES ($1, $2, $3, $4, 'pending') \
             RETURNING id, email, username, password_hash, status, created_at, updated_at",
        )
        .bind(id)
        .bind(&input.email)
        .bind(&input.username)
        .bind(&input.password_hash)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => {
                AppError::Conflict("user already exists".into())
            }
            other => AppError::Internal(other.into()),
        })?;

        Ok(row.into())
    }

    async fn create_with_role(&self, input: CreateUser, role: &str) -> Result<User, AppError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, UserRow>(
            "INSERT INTO users (id, email, username, password_hash, status) \
             VALUES ($1, $2, $3, $4, 'pending') \
             RETURNING id, email, username, password_hash, status, created_at, updated_at",
        )
        .bind(id)
        .bind(&input.email)
        .bind(&input.username)
        .bind(&input.password_hash)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => {
                AppError::Conflict("user already exists".into())
            }
            other => AppError::Internal(other.into()),
        })?;

        let role_result = sqlx::query(
            "INSERT INTO user_roles (user_id, role_id, scope_id) \
             SELECT $1, id, '00000000-0000-0000-0000-000000000000' FROM roles WHERE name = $2",
        )
        .bind(id)
        .bind(role)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if role_result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("role '{role}'")));
        }

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn update_status(&self, id: UserId, status: UserStatus) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE users SET status = $1, updated_at = now() WHERE id = $2",
        )
        .bind(status)
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("user".into()));
        }

        Ok(())
    }

    async fn get_password_hash(&self, id: UserId) -> Result<String, AppError> {
        let row: (String,) = sqlx::query_as(
            "SELECT password_hash FROM users WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("user".into()))?;

        Ok(row.0)
    }
}
