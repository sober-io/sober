//! Core gateway business logic — inbound event routing and outbound delivery.

use std::sync::Arc;

use dashmap::DashMap;
use sober_core::error::AppError;
use sober_core::types::{
    ConversationId, CreateUserMapping, GatewayChannelMapping, MappingId, PlatformId, UserId,
    UserMappingId,
};
use sober_core::types::{GatewayMappingRepo, GatewayUserMappingRepo};
use sober_db::{PgGatewayMappingRepo, PgGatewayUserMappingRepo};
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::agent_proto::{
    ContentBlock, HandleMessageRequest, TextBlock, agent_service_client::AgentServiceClient,
    content_block::Block,
};
use crate::bridge::PlatformBridgeRegistry;
use crate::error::GatewayError;
use crate::types::GatewayEvent;

/// The Sõber bot user UUID — used as the actor for all gateway-initiated messages.
const BOT_USER_UUID: &str = "01960000-0000-7000-8000-000000000100";

/// Core gateway service.
///
/// Maintains in-memory caches of channel and user mappings for fast event
/// routing, backed by PostgreSQL for durability.
pub struct GatewayService {
    db: PgPool,
    agent_client: AgentServiceClient<tonic::transport::Channel>,
    bridge_registry: Arc<PlatformBridgeRegistry>,
    /// Sender for inbound gateway events — passed to each bridge on connect.
    event_tx: mpsc::Sender<GatewayEvent>,

    /// Cache: (platform_id, external_channel_id) → GatewayChannelMapping
    channel_cache: DashMap<(PlatformId, String), GatewayChannelMapping>,
    /// Reverse cache: conversation_id → Vec<(platform_id, external_channel_id)>
    reverse_cache: DashMap<ConversationId, Vec<(PlatformId, String)>>,
    /// User cache: (platform_id, external_user_id) → UserId
    user_cache: DashMap<(PlatformId, String), UserId>,
}

impl GatewayService {
    /// Creates a new gateway service.
    pub fn new(
        db: PgPool,
        agent_client: AgentServiceClient<tonic::transport::Channel>,
        bridge_registry: Arc<PlatformBridgeRegistry>,
        event_tx: mpsc::Sender<GatewayEvent>,
    ) -> Self {
        Self {
            db,
            agent_client,
            bridge_registry,
            event_tx,
            channel_cache: DashMap::new(),
            reverse_cache: DashMap::new(),
            user_cache: DashMap::new(),
        }
    }

    /// Connects all enabled platforms from the database.
    ///
    /// Delegates to [`crate::connector::connect_platforms`].
    pub async fn connect_platforms(&self) -> Result<(), AppError> {
        crate::connector::connect_platforms(&self.db, &self.bridge_registry, &self.event_tx).await
    }

    /// Loads all channel and user mappings from the database into memory.
    pub async fn load_caches(&self) -> Result<(), AppError> {
        info!("loading gateway caches from database");

        let mapping_repo = PgGatewayMappingRepo::new(self.db.clone());
        let user_mapping_repo = PgGatewayUserMappingRepo::new(self.db.clone());

        let mappings = mapping_repo.list_all().await?;
        let user_mappings = user_mapping_repo.list_all().await?;

        self.channel_cache.clear();
        self.reverse_cache.clear();
        self.user_cache.clear();

        for mapping in mappings {
            let key = (mapping.platform_id, mapping.external_channel_id.clone());
            self.channel_cache.insert(key.clone(), mapping.clone());

            self.reverse_cache
                .entry(mapping.conversation_id)
                .or_default()
                .push((mapping.platform_id, mapping.external_channel_id));
        }

        for um in user_mappings {
            let key = (um.platform_id, um.external_user_id.clone());
            self.user_cache.insert(key, um.user_id);
        }

        info!(
            channels = self.channel_cache.len(),
            users = self.user_cache.len(),
            "gateway caches loaded"
        );

        Ok(())
    }

    /// Dispatches an incoming gateway event to the appropriate handler.
    pub async fn handle_event(&self, event: GatewayEvent) {
        match event {
            GatewayEvent::MessageReceived {
                platform_id,
                channel_id,
                user_id,
                username,
                content,
            } => {
                if let Err(e) = self
                    .handle_message(platform_id, channel_id, user_id, username, content)
                    .await
                {
                    error!(error = %e, "failed to handle inbound message");
                    metrics::counter!("sober_gateway_inbound_errors_total").increment(1);
                }
            }
            GatewayEvent::ChannelDeleted {
                platform_id,
                channel_id,
            } => {
                if let Err(e) = self.handle_channel_deleted(platform_id, channel_id).await {
                    error!(error = %e, "failed to handle channel deletion");
                }
            }
        }
    }

    /// Routes an inbound message to the agent via gRPC.
    async fn handle_message(
        &self,
        platform_id: PlatformId,
        channel_id: String,
        external_user_id: String,
        username: String,
        content: String,
    ) -> Result<(), GatewayError> {
        let start = std::time::Instant::now();

        // Look up channel mapping.
        let cache_key = (platform_id, channel_id.clone());
        let mapping = self.channel_cache.get(&cache_key).map(|v| v.clone());

        let mapping = match mapping {
            Some(m) => m,
            None => {
                debug!(
                    platform_id = %platform_id,
                    channel_id = %channel_id,
                    "ignoring message for unmapped channel"
                );
                metrics::counter!("sober_gateway_unmapped_messages_total").increment(1);
                return Ok(());
            }
        };

        // Resolve Sõber user ID for the external user.
        let user_key = (platform_id, external_user_id.clone());
        let user_id = match self.user_cache.get(&user_key).map(|v| *v) {
            Some(uid) => uid,
            None => {
                // Auto-create a user mapping using the bot user as the Sõber user.
                // In production this is managed by the admin — we fall back to the bot user.
                let bot_uuid =
                    Uuid::parse_str(BOT_USER_UUID).expect("BOT_USER_UUID is a valid UUID");
                let bot_user = UserId::from_uuid(bot_uuid);

                // Persist the new user mapping.
                let user_mapping_repo = PgGatewayUserMappingRepo::new(self.db.clone());
                let result = user_mapping_repo
                    .create(
                        UserMappingId::new(),
                        platform_id,
                        &CreateUserMapping {
                            external_user_id: external_user_id.clone(),
                            external_username: username.clone(),
                            user_id: bot_user,
                        },
                    )
                    .await;

                match result {
                    Ok(um) => {
                        self.user_cache.insert(user_key, um.user_id);
                        um.user_id
                    }
                    Err(AppError::Conflict(_)) => {
                        // Created concurrently — just use the bot user.
                        warn!(
                            external_user_id = %external_user_id,
                            "user mapping conflict, using bot user"
                        );
                        self.user_cache.insert(user_key, bot_user);
                        bot_user
                    }
                    Err(e) => return Err(GatewayError::ConnectionFailed(e.to_string())),
                }
            }
        };

        // Forward to agent.
        let request = HandleMessageRequest {
            user_id: user_id.to_string(),
            conversation_id: mapping.conversation_id.to_string(),
            content: vec![ContentBlock {
                block: Some(Block::Text(TextBlock { text: content })),
            }],
        };

        let mut client = self.agent_client.clone();
        client
            .handle_message(request)
            .await
            .map_err(|e| GatewayError::ConnectionFailed(e.to_string()))?;

        let elapsed = start.elapsed().as_secs_f64();
        metrics::histogram!("sober_gateway_message_handle_duration_seconds").record(elapsed);
        metrics::counter!("sober_gateway_messages_received_total", "status" => "success")
            .increment(1);

        debug!(
            conversation_id = %mapping.conversation_id,
            elapsed_ms = elapsed * 1000.0,
            "inbound message forwarded to agent"
        );

        Ok(())
    }

    /// Removes a channel mapping when the external channel is deleted.
    async fn handle_channel_deleted(
        &self,
        platform_id: PlatformId,
        channel_id: String,
    ) -> Result<(), GatewayError> {
        let cache_key = (platform_id, channel_id.clone());

        // Remove from cache.
        if let Some((_, mapping)) = self.channel_cache.remove(&cache_key) {
            // Remove from reverse cache.
            if let Some(mut targets) = self.reverse_cache.get_mut(&mapping.conversation_id) {
                targets.retain(|(pid, cid)| !(*pid == platform_id && *cid == channel_id));
            }
        }

        // Remove from DB.
        let mapping_repo = PgGatewayMappingRepo::new(self.db.clone());
        mapping_repo
            .delete_by_external_channel(platform_id, &channel_id)
            .await
            .map_err(|e| GatewayError::ConnectionFailed(e.to_string()))?;

        info!(
            platform_id = %platform_id,
            channel_id = %channel_id,
            "channel mapping removed"
        );

        Ok(())
    }

    /// Returns true if the given user ID is a gateway-mapped external user.
    ///
    /// Used to avoid echoing messages back to the platform they came from.
    pub fn is_gateway_user(&self, user_id: &UserId) -> bool {
        self.user_cache.iter().any(|entry| entry.value() == user_id)
    }

    /// Returns all `(platform_id, channel_id)` pairs that should receive
    /// outbound messages for the given conversation.
    pub fn get_outbound_targets(
        &self,
        conversation_id: &ConversationId,
    ) -> Vec<(PlatformId, String)> {
        self.reverse_cache
            .get(conversation_id)
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    /// Returns the bridge registry.
    pub fn bridge_registry(&self) -> &Arc<PlatformBridgeRegistry> {
        &self.bridge_registry
    }

    /// Invalidates and reloads all caches from the database, then reconnects platforms.
    pub async fn reload(&self) -> Result<(), AppError> {
        self.load_caches().await?;
        self.bridge_registry.clear();
        self.connect_platforms().await?;
        Ok(())
    }

    /// Inserts a mapping into the in-memory caches.
    ///
    /// Used by the admin service when creating a new mapping without a full reload.
    pub fn insert_mapping_cache(&self, mapping: GatewayChannelMapping) {
        let key = (mapping.platform_id, mapping.external_channel_id.clone());
        self.reverse_cache
            .entry(mapping.conversation_id)
            .or_default()
            .push((mapping.platform_id, mapping.external_channel_id.clone()));
        self.channel_cache.insert(key, mapping);
    }

    /// Removes a mapping from the in-memory caches by its mapping ID.
    pub fn remove_mapping_cache(&self, mapping_id: MappingId) {
        // Find and remove from channel cache.
        let to_remove: Vec<(PlatformId, String)> = self
            .channel_cache
            .iter()
            .filter(|entry| entry.value().id == mapping_id)
            .map(|entry| entry.key().clone())
            .collect();

        for key in to_remove {
            if let Some((_, mapping)) = self.channel_cache.remove(&key)
                && let Some(mut targets) = self.reverse_cache.get_mut(&mapping.conversation_id)
            {
                targets.retain(|(pid, cid)| {
                    !(*pid == mapping.platform_id && *cid == mapping.external_channel_id)
                });
            }
        }
    }
}
