//! PostgreSQL implementation of [`SecretRepo`].

use sober_core::error::AppError;
use sober_core::types::{
    ConversationId, NewSecret, SecretId, SecretMetadata, SecretRow, StoredDek, UpdateSecret, UserId,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::rows::{EncryptionKeyRow, SecretDbRow, SecretMetadataRow};

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
    async fn get_dek(&self, user_id: UserId) -> Result<Option<StoredDek>, AppError> {
        let row = sqlx::query_as::<_, EncryptionKeyRow>(
            "SELECT id, user_id, encrypted_dek, mek_version, created_at, rotated_at \
             FROM encryption_keys WHERE user_id = $1",
        )
        .bind(user_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.map(Into::into))
    }

    async fn store_dek(
        &self,
        user_id: UserId,
        encrypted_dek: Vec<u8>,
        mek_version: i32,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO encryption_keys (user_id, encrypted_dek, mek_version) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (user_id) \
             DO UPDATE SET encrypted_dek = $2, mek_version = $3, rotated_at = now()",
        )
        .bind(user_id.as_uuid())
        .bind(&encrypted_dek)
        .bind(mek_version)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn list_secrets(
        &self,
        user_id: UserId,
        conversation_id: Option<ConversationId>,
        secret_type: Option<&str>,
    ) -> Result<Vec<SecretMetadata>, AppError> {
        let conv_id = conversation_id.map(|c| *c.as_uuid());
        let rows = sqlx::query_as::<_, SecretMetadataRow>(
            "SELECT id, user_id, name, secret_type, metadata, priority, \
                    conversation_id, created_at, updated_at \
             FROM secrets \
             WHERE user_id = $1 \
               AND ($2::uuid IS NULL OR conversation_id = $2 OR conversation_id IS NULL) \
               AND ($3::text IS NULL OR secret_type = $3) \
             ORDER BY \
               CASE WHEN conversation_id IS NOT NULL THEN 0 ELSE 1 END, \
               priority ASC NULLS LAST",
        )
        .bind(user_id.as_uuid())
        .bind(conv_id)
        .bind(secret_type)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_secret(&self, id: SecretId) -> Result<Option<SecretRow>, AppError> {
        let row = sqlx::query_as::<_, SecretDbRow>(
            "SELECT id, user_id, name, secret_type, metadata, encrypted_data, \
                    conversation_id, priority, created_at, updated_at \
             FROM secrets WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.map(Into::into))
    }

    async fn get_secret_by_name(
        &self,
        user_id: UserId,
        conversation_id: Option<ConversationId>,
        name: &str,
    ) -> Result<Option<SecretRow>, AppError> {
        let conv_id = conversation_id.map(|c| *c.as_uuid());
        let row = sqlx::query_as::<_, SecretDbRow>(
            "SELECT id, user_id, name, secret_type, metadata, encrypted_data, \
                    conversation_id, priority, created_at, updated_at \
             FROM secrets \
             WHERE user_id = $1 AND name = $3 \
               AND (conversation_id = $2 OR ($2::uuid IS NULL AND conversation_id IS NULL)) \
             ORDER BY CASE WHEN conversation_id IS NOT NULL THEN 0 ELSE 1 END \
             LIMIT 1",
        )
        .bind(user_id.as_uuid())
        .bind(conv_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.map(Into::into))
    }

    async fn store_secret(&self, secret: NewSecret) -> Result<SecretId, AppError> {
        let id = Uuid::now_v7();
        let conv_id = secret.conversation_id.map(|c| *c.as_uuid());
        sqlx::query(
            "INSERT INTO secrets (id, user_id, name, secret_type, metadata, \
                                  encrypted_data, conversation_id, priority) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(id)
        .bind(secret.user_id.as_uuid())
        .bind(&secret.name)
        .bind(&secret.secret_type)
        .bind(&secret.metadata)
        .bind(&secret.encrypted_data)
        .bind(conv_id)
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
            "UPDATE secrets SET {} WHERE id = $1",
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
        sqlx::query("DELETE FROM secrets WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn list_secret_ids(
        &self,
        user_id: UserId,
        conversation_id: Option<ConversationId>,
    ) -> Result<Vec<SecretId>, AppError> {
        let conv_id = conversation_id.map(|c| *c.as_uuid());

        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM secrets \
             WHERE user_id = $1 \
               AND ($2::uuid IS NULL OR conversation_id = $2 OR conversation_id IS NULL) \
             ORDER BY created_at ASC",
        )
        .bind(user_id.as_uuid())
        .bind(conv_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows
            .into_iter()
            .map(|(id,)| SecretId::from_uuid(id))
            .collect())
    }
}
