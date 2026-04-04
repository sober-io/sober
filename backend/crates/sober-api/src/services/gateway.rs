//! Gateway admin service — platform, channel mapping, and user mapping CRUD.

use sober_core::error::AppError;
use sober_core::types::{
    CreateChannelMapping, CreatePlatform, CreateUserMapping, GatewayChannelMapping,
    GatewayMappingRepo, GatewayPlatform, GatewayPlatformRepo, GatewayUserMapping,
    GatewayUserMappingRepo, MappingId, PlatformId, UpdatePlatform, UserMappingId,
};
use sober_db::{PgGatewayMappingRepo, PgGatewayPlatformRepo, PgGatewayUserMappingRepo};
use sqlx::PgPool;
use tracing::instrument;
use uuid::Uuid;

/// The bridge bot user UUID — added as a member of every mapped conversation.
const BRIDGE_BOT_USER_ID: Uuid = uuid::uuid!("01960000-0000-7000-8000-000000000100");

/// Service for managing gateway platforms, channel mappings, and user mappings.
pub struct GatewayAdminService {
    db: PgPool,
}

impl GatewayAdminService {
    /// Creates a new service backed by the given connection pool.
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    // -----------------------------------------------------------------------
    // Platforms
    // -----------------------------------------------------------------------

    /// Lists all gateway platforms (enabled and disabled).
    #[instrument(level = "debug", skip(self))]
    pub async fn list_platforms(&self) -> Result<Vec<GatewayPlatform>, AppError> {
        let repo = PgGatewayPlatformRepo::new(self.db.clone());
        repo.list(false).await
    }

    /// Returns a single gateway platform by ID.
    #[instrument(level = "debug", skip(self), fields(platform.id = %id))]
    pub async fn get_platform(&self, id: PlatformId) -> Result<GatewayPlatform, AppError> {
        let repo = PgGatewayPlatformRepo::new(self.db.clone());
        repo.get(id).await
    }

    /// Creates a new gateway platform.
    #[instrument(level = "debug", skip(self, input))]
    pub async fn create_platform(
        &self,
        input: CreatePlatform,
    ) -> Result<GatewayPlatform, AppError> {
        let repo = PgGatewayPlatformRepo::new(self.db.clone());
        let id = PlatformId::new();
        repo.create(id, &input).await
    }

    /// Updates an existing gateway platform.
    #[instrument(level = "debug", skip(self, input), fields(platform.id = %id))]
    pub async fn update_platform(
        &self,
        id: PlatformId,
        input: UpdatePlatform,
    ) -> Result<GatewayPlatform, AppError> {
        let repo = PgGatewayPlatformRepo::new(self.db.clone());
        repo.update(id, &input).await
    }

    /// Deletes a gateway platform.
    #[instrument(level = "debug", skip(self), fields(platform.id = %id))]
    pub async fn delete_platform(&self, id: PlatformId) -> Result<(), AppError> {
        let repo = PgGatewayPlatformRepo::new(self.db.clone());
        repo.delete(id).await
    }

    /// Stores plaintext credentials for a gateway platform as JSONB.
    #[instrument(level = "debug", skip(self, credentials), fields(platform.id = %id))]
    pub async fn store_platform_credentials(
        &self,
        id: PlatformId,
        credentials: serde_json::Value,
    ) -> Result<(), AppError> {
        let repo = PgGatewayPlatformRepo::new(self.db.clone());
        // Verify platform exists.
        repo.get(id).await?;
        repo.store_credentials(id, &credentials).await
    }

    // -----------------------------------------------------------------------
    // Channel mappings
    // -----------------------------------------------------------------------

    /// Lists all channel mappings for a platform.
    #[instrument(level = "debug", skip(self), fields(platform.id = %platform_id))]
    pub async fn list_mappings(
        &self,
        platform_id: PlatformId,
    ) -> Result<Vec<GatewayChannelMapping>, AppError> {
        let repo = PgGatewayMappingRepo::new(self.db.clone());
        repo.list_by_platform(platform_id).await
    }

    /// Creates a channel mapping, then adds the bridge bot as a conversation member.
    ///
    /// Both operations run in a single transaction.
    #[instrument(level = "debug", skip(self, input), fields(platform.id = %platform_id))]
    pub async fn create_mapping(
        &self,
        platform_id: PlatformId,
        input: CreateChannelMapping,
    ) -> Result<GatewayChannelMapping, AppError> {
        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let id = MappingId::new();
        let mapping = PgGatewayMappingRepo::create_tx(&mut tx, id, platform_id, &input).await?;

        // Add the bridge bot as a member of the mapped conversation.
        let bot_user_id = sober_core::types::UserId::from_uuid(BRIDGE_BOT_USER_ID);
        PgGatewayMappingRepo::add_user_to_conversation_tx(
            &mut tx,
            input.conversation_id,
            bot_user_id,
        )
        .await?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(mapping)
    }

    /// Deletes a channel mapping by ID.
    #[instrument(level = "debug", skip(self), fields(mapping.id = %id))]
    pub async fn delete_mapping(&self, id: MappingId) -> Result<(), AppError> {
        let repo = PgGatewayMappingRepo::new(self.db.clone());
        repo.delete(id).await
    }

    // -----------------------------------------------------------------------
    // User mappings
    // -----------------------------------------------------------------------

    /// Lists all user mappings for a platform.
    #[instrument(level = "debug", skip(self), fields(platform.id = %platform_id))]
    pub async fn list_user_mappings(
        &self,
        platform_id: PlatformId,
    ) -> Result<Vec<GatewayUserMapping>, AppError> {
        let repo = PgGatewayUserMappingRepo::new(self.db.clone());
        repo.list_by_platform(platform_id).await
    }

    /// Creates a user mapping, then adds the Sõber user to all conversations mapped for this platform.
    ///
    /// Both operations run in a single transaction.
    #[instrument(level = "debug", skip(self, input), fields(platform.id = %platform_id))]
    pub async fn create_user_mapping(
        &self,
        platform_id: PlatformId,
        input: CreateUserMapping,
    ) -> Result<GatewayUserMapping, AppError> {
        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let id = UserMappingId::new();
        let mapping = PgGatewayUserMappingRepo::create_tx(&mut tx, id, platform_id, &input).await?;

        // Add the Sõber user to all conversations currently mapped for this platform.
        PgGatewayMappingRepo::add_user_to_mapped_conversations_tx(
            &mut tx,
            input.user_id,
            platform_id,
        )
        .await?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        Ok(mapping)
    }

    /// Deletes a user mapping by ID.
    #[instrument(level = "debug", skip(self), fields(user_mapping.id = %id))]
    pub async fn delete_user_mapping(&self, id: UserMappingId) -> Result<(), AppError> {
        let repo = PgGatewayUserMappingRepo::new(self.db.clone());
        repo.delete(id).await
    }
}
