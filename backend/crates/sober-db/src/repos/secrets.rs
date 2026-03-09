//! PostgreSQL implementation of [`SecretRepo`].

use sober_core::error::AppError;
use sober_core::types::{
    NewSecret, SecretId, SecretMetadata, SecretRow, SecretScope, StoredDek, UpdateSecret,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::{EncryptionKeyRow, SecretMetadataRow, UserSecretRow};

/// PostgreSQL-backed secret repository.
pub struct PgSecretRepo {
    pool: PgPool,
}

impl PgSecretRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::SecretRepo for PgSecretRepo {
    async fn get_dek(&self, scope: SecretScope) -> Result<Option<StoredDek>, AppError> {
        let SecretScope::User(uid) = scope;
        let row = sqlx::query_as::<_, EncryptionKeyRow>(
            "SELECT id, user_id, encrypted_dek, mek_version, created_at, rotated_at \
             FROM encryption_keys WHERE user_id = $1",
        )
        .bind(uid.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.map(Into::into))
    }

    async fn store_dek(
        &self,
        scope: SecretScope,
        encrypted_dek: Vec<u8>,
        mek_version: i32,
    ) -> Result<(), AppError> {
        let SecretScope::User(uid) = scope;
        sqlx::query(
            "INSERT INTO encryption_keys (user_id, encrypted_dek, mek_version) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (user_id) \
             DO UPDATE SET encrypted_dek = $2, mek_version = $3, rotated_at = now()",
        )
        .bind(uid.as_uuid())
        .bind(&encrypted_dek)
        .bind(mek_version)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn list_secrets(
        &self,
        scope: SecretScope,
        secret_type: Option<&str>,
    ) -> Result<Vec<SecretMetadata>, AppError> {
        let SecretScope::User(uid) = scope;
        let rows = match secret_type {
            Some(st) => {
                sqlx::query_as::<_, SecretMetadataRow>(
                    "SELECT id, name, secret_type, metadata, priority, created_at, updated_at \
                     FROM user_secrets \
                     WHERE user_id = $1 AND secret_type = $2 \
                     ORDER BY priority ASC NULLS LAST, created_at ASC",
                )
                .bind(uid.as_uuid())
                .bind(st)
                .fetch_all(&self.pool)
                .await
            }
            None => {
                sqlx::query_as::<_, SecretMetadataRow>(
                    "SELECT id, name, secret_type, metadata, priority, created_at, updated_at \
                     FROM user_secrets \
                     WHERE user_id = $1 \
                     ORDER BY priority ASC NULLS LAST, created_at ASC",
                )
                .bind(uid.as_uuid())
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_secret(&self, id: SecretId) -> Result<Option<SecretRow>, AppError> {
        let row = sqlx::query_as::<_, UserSecretRow>(
            "SELECT id, user_id, name, secret_type, metadata, encrypted_data, \
                    priority, created_at, updated_at \
             FROM user_secrets WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.map(Into::into))
    }

    async fn get_secret_by_name(
        &self,
        scope: SecretScope,
        name: &str,
    ) -> Result<Option<SecretRow>, AppError> {
        let SecretScope::User(uid) = scope;
        let row = sqlx::query_as::<_, UserSecretRow>(
            "SELECT id, user_id, name, secret_type, metadata, encrypted_data, \
                    priority, created_at, updated_at \
             FROM user_secrets WHERE user_id = $1 AND name = $2",
        )
        .bind(uid.as_uuid())
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.map(Into::into))
    }

    async fn store_secret(&self, secret: NewSecret) -> Result<SecretId, AppError> {
        let SecretScope::User(uid) = secret.scope;
        let id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO user_secrets (id, user_id, name, secret_type, metadata, encrypted_data, priority) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(id)
        .bind(uid.as_uuid())
        .bind(&secret.name)
        .bind(&secret.secret_type)
        .bind(&secret.metadata)
        .bind(&secret.encrypted_data)
        .bind(secret.priority)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(SecretId::from_uuid(id))
    }

    async fn update_secret(&self, id: SecretId, update: UpdateSecret) -> Result<(), AppError> {
        // Build dynamic UPDATE — only set fields that are Some.
        let mut set_clauses = Vec::new();
        let mut param_idx = 2u32; // $1 is the id

        if update.name.is_some() {
            set_clauses.push(format!("name = ${param_idx}"));
            param_idx += 1;
        }
        if update.metadata.is_some() {
            set_clauses.push(format!("metadata = ${param_idx}"));
            param_idx += 1;
        }
        if update.encrypted_data.is_some() {
            set_clauses.push(format!("encrypted_data = ${param_idx}"));
            param_idx += 1;
        }
        if update.priority.is_some() {
            set_clauses.push(format!("priority = ${param_idx}"));
            // param_idx += 1; // last one, no need to increment
        }

        if set_clauses.is_empty() {
            return Ok(()); // nothing to update
        }

        set_clauses.push("updated_at = now()".to_string());
        let sql = format!(
            "UPDATE user_secrets SET {} WHERE id = $1",
            set_clauses.join(", ")
        );

        // We must bind parameters in order. Use a query builder approach.
        let mut query = sqlx::query(&sql).bind(id.as_uuid());

        if let Some(ref name) = update.name {
            query = query.bind(name);
        }
        if let Some(ref metadata) = update.metadata {
            query = query.bind(metadata);
        }
        if let Some(ref encrypted_data) = update.encrypted_data {
            query = query.bind(encrypted_data);
        }
        if let Some(priority) = update.priority {
            query = query.bind(priority);
        }

        query
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn delete_secret(&self, id: SecretId) -> Result<(), AppError> {
        sqlx::query("DELETE FROM user_secrets WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn list_secret_ids(&self, scope: SecretScope) -> Result<Vec<SecretId>, AppError> {
        let SecretScope::User(uid) = scope;

        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM user_secrets WHERE user_id = $1 ORDER BY created_at ASC",
        )
        .bind(uid.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows
            .into_iter()
            .map(|(id,)| SecretId::from_uuid(id))
            .collect())
    }
}
