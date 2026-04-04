//! PostgreSQL implementations of gateway repository traits.

use sober_core::error::AppError;
use sober_core::types::{
    ConversationId, CreateChannelMapping, CreatePlatform, CreateUserMapping, GatewayChannelMapping,
    GatewayPlatform, GatewayUserMapping, MappingId, PlatformId, UpdatePlatform, UserId,
    UserMappingId,
};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::rows::{GatewayChannelMappingRow, GatewayPlatformRow, GatewayUserMappingRow};

// ---------------------------------------------------------------------------
// Platform repo
// ---------------------------------------------------------------------------

/// PostgreSQL-backed gateway platform repository.
#[derive(Clone)]
pub struct PgGatewayPlatformRepo {
    pool: PgPool,
}

impl PgGatewayPlatformRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl sober_core::types::GatewayPlatformRepo for PgGatewayPlatformRepo {
    async fn list(&self, enabled_only: bool) -> Result<Vec<GatewayPlatform>, AppError> {
        let rows = if enabled_only {
            sqlx::query_as::<_, GatewayPlatformRow>(
                "SELECT id, platform_type, display_name, is_enabled, created_at, updated_at \
                 FROM gateway_platforms \
                 WHERE is_enabled = true \
                 ORDER BY created_at ASC",
            )
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, GatewayPlatformRow>(
                "SELECT id, platform_type, display_name, is_enabled, created_at, updated_at \
                 FROM gateway_platforms \
                 ORDER BY created_at ASC",
            )
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get(&self, id: PlatformId) -> Result<GatewayPlatform, AppError> {
        let row = sqlx::query_as::<_, GatewayPlatformRow>(
            "SELECT id, platform_type, display_name, is_enabled, created_at, updated_at \
             FROM gateway_platforms \
             WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("gateway platform".into()))?;

        Ok(row.into())
    }

    async fn create(
        &self,
        id: PlatformId,
        input: &CreatePlatform,
    ) -> Result<GatewayPlatform, AppError> {
        let row = sqlx::query_as::<_, GatewayPlatformRow>(
            "INSERT INTO gateway_platforms \
             (id, platform_type, display_name) \
             VALUES ($1, $2, $3) \
             RETURNING id, platform_type, display_name, is_enabled, created_at, updated_at",
        )
        .bind(id.as_uuid())
        .bind(input.platform_type.to_string())
        .bind(&input.display_name)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }

    async fn update(
        &self,
        id: PlatformId,
        input: &UpdatePlatform,
    ) -> Result<GatewayPlatform, AppError> {
        let row = sqlx::query_as::<_, GatewayPlatformRow>(
            "UPDATE gateway_platforms \
             SET display_name = COALESCE($2, display_name), \
                 is_enabled   = COALESCE($3, is_enabled), \
                 updated_at   = now() \
             WHERE id = $1 \
             RETURNING id, platform_type, display_name, is_enabled, created_at, updated_at",
        )
        .bind(id.as_uuid())
        .bind(input.display_name.as_deref())
        .bind(input.is_enabled)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("gateway platform".into()))?;

        Ok(row.into())
    }

    async fn delete(&self, id: PlatformId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM gateway_platforms WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("gateway platform".into()));
        }

        Ok(())
    }

    async fn store_credentials(
        &self,
        id: PlatformId,
        credentials: &serde_json::Value,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE gateway_platforms SET credentials = $1, updated_at = now() WHERE id = $2",
        )
        .bind(credentials)
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn get_credentials(&self, id: PlatformId) -> Result<Option<serde_json::Value>, AppError> {
        let row: Option<(Option<serde_json::Value>,)> =
            sqlx::query_as("SELECT credentials FROM gateway_platforms WHERE id = $1")
                .bind(id.as_uuid())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.and_then(|r| r.0))
    }
}

// ---------------------------------------------------------------------------
// Channel mapping repo
// ---------------------------------------------------------------------------

/// PostgreSQL-backed gateway channel mapping repository.
#[derive(Clone)]
pub struct PgGatewayMappingRepo {
    pool: PgPool,
}

impl PgGatewayMappingRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Adds a user as a member to all conversations mapped for a platform.
    pub async fn add_user_to_mapped_conversations_tx(
        conn: &mut PgConnection,
        user_id: sober_core::types::UserId,
        platform_id: PlatformId,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO conversation_users \
             (conversation_id, user_id, role, unread_count, last_read_at, joined_at) \
             SELECT gcm.conversation_id, $1, 'member', 0, now(), now() \
             FROM gateway_channel_mappings gcm \
             WHERE gcm.platform_id = $2 \
             ON CONFLICT (conversation_id, user_id) DO NOTHING",
        )
        .bind(user_id.as_uuid())
        .bind(platform_id.as_uuid())
        .execute(&mut *conn)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        Ok(())
    }

    /// Adds a user as a member to a single conversation.
    pub async fn add_user_to_conversation_tx(
        conn: &mut PgConnection,
        conversation_id: ConversationId,
        user_id: sober_core::types::UserId,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO conversation_users \
             (conversation_id, user_id, role, unread_count, last_read_at, joined_at) \
             VALUES ($1, $2, 'member', 0, now(), now()) \
             ON CONFLICT (conversation_id, user_id) DO NOTHING",
        )
        .bind(conversation_id.as_uuid())
        .bind(user_id.as_uuid())
        .execute(&mut *conn)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        Ok(())
    }

    /// Creates a channel mapping within an existing transaction.
    pub async fn create_tx(
        conn: &mut PgConnection,
        id: MappingId,
        platform_id: PlatformId,
        input: &CreateChannelMapping,
    ) -> Result<GatewayChannelMapping, AppError> {
        let row = sqlx::query_as::<_, GatewayChannelMappingRow>(
            "INSERT INTO gateway_channel_mappings \
             (id, platform_id, external_channel_id, external_channel_name, conversation_id) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING id, platform_id, external_channel_id, external_channel_name, \
                       conversation_id, is_thread, parent_mapping_id, created_at",
        )
        .bind(id.as_uuid())
        .bind(platform_id.as_uuid())
        .bind(&input.external_channel_id)
        .bind(&input.external_channel_name)
        .bind(input.conversation_id.as_uuid())
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => {
                AppError::Conflict("a mapping for this channel already exists".into())
            }
            other => AppError::Internal(other.into()),
        })?;

        Ok(row.into())
    }
}

impl sober_core::types::GatewayMappingRepo for PgGatewayMappingRepo {
    async fn list_by_platform(
        &self,
        platform_id: PlatformId,
    ) -> Result<Vec<GatewayChannelMapping>, AppError> {
        let rows = sqlx::query_as::<_, GatewayChannelMappingRow>(
            "SELECT id, platform_id, external_channel_id, external_channel_name, \
                    conversation_id, is_thread, parent_mapping_id, created_at \
             FROM gateway_channel_mappings \
             WHERE platform_id = $1 \
             ORDER BY created_at ASC",
        )
        .bind(platform_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn find_by_external_channel(
        &self,
        platform_id: PlatformId,
        external_channel_id: &str,
    ) -> Result<Option<GatewayChannelMapping>, AppError> {
        let row = sqlx::query_as::<_, GatewayChannelMappingRow>(
            "SELECT id, platform_id, external_channel_id, external_channel_name, \
                    conversation_id, is_thread, parent_mapping_id, created_at \
             FROM gateway_channel_mappings \
             WHERE platform_id = $1 AND external_channel_id = $2",
        )
        .bind(platform_id.as_uuid())
        .bind(external_channel_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.map(Into::into))
    }

    async fn find_by_conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<Vec<GatewayChannelMapping>, AppError> {
        let rows = sqlx::query_as::<_, GatewayChannelMappingRow>(
            "SELECT id, platform_id, external_channel_id, external_channel_name, \
                    conversation_id, is_thread, parent_mapping_id, created_at \
             FROM gateway_channel_mappings \
             WHERE conversation_id = $1 \
             ORDER BY created_at ASC",
        )
        .bind(conversation_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn create(
        &self,
        id: MappingId,
        platform_id: PlatformId,
        input: &CreateChannelMapping,
    ) -> Result<GatewayChannelMapping, AppError> {
        let row = sqlx::query_as::<_, GatewayChannelMappingRow>(
            "INSERT INTO gateway_channel_mappings \
             (id, platform_id, external_channel_id, external_channel_name, conversation_id) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING id, platform_id, external_channel_id, external_channel_name, \
                       conversation_id, is_thread, parent_mapping_id, created_at",
        )
        .bind(id.as_uuid())
        .bind(platform_id.as_uuid())
        .bind(&input.external_channel_id)
        .bind(&input.external_channel_name)
        .bind(input.conversation_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => {
                AppError::Conflict("a mapping for this channel already exists".into())
            }
            other => AppError::Internal(other.into()),
        })?;

        Ok(row.into())
    }

    async fn delete(&self, id: MappingId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM gateway_channel_mappings WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("gateway channel mapping".into()));
        }

        Ok(())
    }

    async fn delete_by_external_channel(
        &self,
        platform_id: PlatformId,
        external_channel_id: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "DELETE FROM gateway_channel_mappings \
             WHERE platform_id = $1 AND external_channel_id = $2",
        )
        .bind(platform_id.as_uuid())
        .bind(external_channel_id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<GatewayChannelMapping>, AppError> {
        let rows = sqlx::query_as::<_, GatewayChannelMappingRow>(
            "SELECT id, platform_id, external_channel_id, external_channel_name, \
                    conversation_id, is_thread, parent_mapping_id, created_at \
             FROM gateway_channel_mappings \
             ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_conversation_owner(
        &self,
        conversation_id: ConversationId,
    ) -> Result<UserId, AppError> {
        let row: Option<(Uuid,)> =
            sqlx::query_as("SELECT user_id FROM conversations WHERE id = $1")
                .bind(conversation_id.as_uuid())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        row.map(|(uuid,)| UserId::from_uuid(uuid))
            .ok_or_else(|| AppError::NotFound("conversation not found".into()))
    }
}

// ---------------------------------------------------------------------------
// User mapping repo
// ---------------------------------------------------------------------------

/// PostgreSQL-backed gateway user mapping repository.
#[derive(Clone)]
pub struct PgGatewayUserMappingRepo {
    pool: PgPool,
}

impl PgGatewayUserMappingRepo {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Creates a user mapping within an existing transaction.
    pub async fn create_tx(
        conn: &mut PgConnection,
        id: UserMappingId,
        platform_id: PlatformId,
        input: &CreateUserMapping,
    ) -> Result<GatewayUserMapping, AppError> {
        let row = sqlx::query_as::<_, GatewayUserMappingRow>(
            "INSERT INTO gateway_user_mappings \
             (id, platform_id, external_user_id, external_username, user_id) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING id, platform_id, external_user_id, external_username, user_id, created_at",
        )
        .bind(id.as_uuid())
        .bind(platform_id.as_uuid())
        .bind(&input.external_user_id)
        .bind(&input.external_username)
        .bind(input.user_id.as_uuid())
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => {
                AppError::Conflict("a mapping for this external user already exists".into())
            }
            other => AppError::Internal(other.into()),
        })?;

        Ok(row.into())
    }
}

impl sober_core::types::GatewayUserMappingRepo for PgGatewayUserMappingRepo {
    async fn list_by_platform(
        &self,
        platform_id: PlatformId,
    ) -> Result<Vec<GatewayUserMapping>, AppError> {
        let rows = sqlx::query_as::<_, GatewayUserMappingRow>(
            "SELECT id, platform_id, external_user_id, external_username, user_id, created_at \
             FROM gateway_user_mappings \
             WHERE platform_id = $1 \
             ORDER BY created_at ASC",
        )
        .bind(platform_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn find_by_external_user(
        &self,
        platform_id: PlatformId,
        external_user_id: &str,
    ) -> Result<Option<GatewayUserMapping>, AppError> {
        let row = sqlx::query_as::<_, GatewayUserMappingRow>(
            "SELECT id, platform_id, external_user_id, external_username, user_id, created_at \
             FROM gateway_user_mappings \
             WHERE platform_id = $1 AND external_user_id = $2",
        )
        .bind(platform_id.as_uuid())
        .bind(external_user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.map(Into::into))
    }

    async fn create(
        &self,
        id: UserMappingId,
        platform_id: PlatformId,
        input: &CreateUserMapping,
    ) -> Result<GatewayUserMapping, AppError> {
        let row = sqlx::query_as::<_, GatewayUserMappingRow>(
            "INSERT INTO gateway_user_mappings \
             (id, platform_id, external_user_id, external_username, user_id) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING id, platform_id, external_user_id, external_username, user_id, created_at",
        )
        .bind(id.as_uuid())
        .bind(platform_id.as_uuid())
        .bind(&input.external_user_id)
        .bind(&input.external_username)
        .bind(input.user_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => {
                AppError::Conflict("a mapping for this external user already exists".into())
            }
            other => AppError::Internal(other.into()),
        })?;

        Ok(row.into())
    }

    async fn delete(&self, id: UserMappingId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM gateway_user_mappings WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("gateway user mapping".into()));
        }

        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<GatewayUserMapping>, AppError> {
        let rows = sqlx::query_as::<_, GatewayUserMappingRow>(
            "SELECT id, platform_id, external_user_id, external_username, user_id, created_at \
             FROM gateway_user_mappings \
             ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }
}
