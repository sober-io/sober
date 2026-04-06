//! Core gateway business logic — inbound event routing and outbound delivery.

use std::sync::Arc;

use dashmap::DashMap;
use sober_core::error::AppError;
use sober_core::types::{
    AttachmentKind, ConversationId, GatewayChannelMapping, MappingId, PlatformId, UserId,
};
use sober_core::types::{GatewayMappingRepo, GatewayUserMappingRepo};
use sober_db::{PgGatewayMappingRepo, PgGatewayUserMappingRepo};
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use sober_workspace::BlobStore;

use crate::agent_proto::{
    AudioBlock, ContentBlock, FileBlock, HandleMessageRequest, ImageBlock, TextBlock, VideoBlock,
    agent_service_client::AgentServiceClient, content_block::Block,
};
use crate::bridge::PlatformBridgeRegistry;
use crate::error::GatewayError;
use crate::types::GatewayEvent;

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
    /// Blob store for direct attachment storage/retrieval.
    blob_store: Arc<BlobStore>,

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
        blob_store: Arc<BlobStore>,
    ) -> Self {
        Self {
            db,
            agent_client,
            bridge_registry,
            event_tx,
            blob_store,
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
                attachments,
            } => {
                if let Err(e) = self
                    .handle_message(
                        platform_id,
                        channel_id,
                        user_id,
                        username,
                        content,
                        attachments,
                    )
                    .await
                {
                    error!(error = %e, platform_id = %platform_id, "failed to handle inbound message");
                    metrics::counter!("sober_gateway_inbound_errors_total", "platform" => platform_id.to_string()).increment(1);
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
        _username: String,
        content: String,
        attachments: Vec<crate::types::InboundAttachment>,
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
                metrics::counter!("sober_gateway_unmapped_messages_total", "platform" => platform_id.to_string()).increment(1);
                return Ok(());
            }
        };

        // Resolve Sõber user ID for the external user.
        let user_key = (platform_id, external_user_id.clone());
        let user_id = match self.user_cache.get(&user_key).map(|v| *v) {
            Some(uid) => uid,
            None => {
                let owner_id = self
                    .resolve_conversation_owner(mapping.conversation_id)
                    .await?;
                self.user_cache.insert(user_key, owner_id);
                owner_id
            }
        };

        // Build content blocks: text first, then stored attachments.
        let mut content_blocks = Vec::new();

        if !content.is_empty() {
            content_blocks.push(ContentBlock {
                block: Some(Block::Text(TextBlock { text: content })),
            });
        }

        let platform_label = self
            .bridge_registry
            .get(&platform_id)
            .map(|b| b.platform_type().to_string())
            .unwrap_or_else(|| "unknown".to_owned());

        for attachment in attachments {
            match sober_workspace::attachment::process_and_store_attachment(
                &self.db,
                &self.blob_store,
                mapping.conversation_id,
                user_id,
                attachment.filename.clone(),
                attachment.data,
            )
            .await
            {
                Ok(stored) => {
                    let block = match stored.kind {
                        AttachmentKind::Image => Block::Image(ImageBlock {
                            conversation_attachment_id: stored.id.to_string(),
                            alt: Some(attachment.filename),
                        }),
                        AttachmentKind::Audio => Block::Audio(AudioBlock {
                            conversation_attachment_id: stored.id.to_string(),
                        }),
                        AttachmentKind::Video => Block::Video(VideoBlock {
                            conversation_attachment_id: stored.id.to_string(),
                        }),
                        AttachmentKind::Document => Block::File(FileBlock {
                            conversation_attachment_id: stored.id.to_string(),
                        }),
                    };
                    content_blocks.push(ContentBlock { block: Some(block) });

                    let kind_label = match stored.kind {
                        AttachmentKind::Image => "image",
                        AttachmentKind::Audio => "audio",
                        AttachmentKind::Video => "video",
                        AttachmentKind::Document => "document",
                    };
                    metrics::counter!(
                        "sober_gateway_attachments_stored_total",
                        "platform" => platform_label.clone(),
                        "kind" => kind_label,
                        "status" => "success",
                    )
                    .increment(1);
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        filename = %attachment.filename,
                        "failed to store attachment, skipping"
                    );
                }
            }
        }

        // Skip if there's nothing to send (no text and all attachments failed).
        if content_blocks.is_empty() {
            return Ok(());
        }

        // Forward to agent.
        let request = HandleMessageRequest {
            user_id: user_id.to_string(),
            conversation_id: mapping.conversation_id.to_string(),
            content: content_blocks,
            source: "gateway".to_owned(),
        };

        let mut client = self.agent_client.clone();
        client
            .handle_message(request)
            .await
            .map_err(|e| GatewayError::ConnectionFailed(e.to_string()))?;

        let elapsed = start.elapsed().as_secs_f64();
        metrics::histogram!("sober_gateway_message_handle_duration_seconds", "platform" => platform_label.clone()).record(elapsed);
        metrics::counter!("sober_gateway_messages_received_total", "platform" => platform_label, "status" => "success")
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

    /// Resolves the owner (creator) of a conversation via the mapping repo.
    async fn resolve_conversation_owner(
        &self,
        conversation_id: ConversationId,
    ) -> Result<UserId, GatewayError> {
        let repo = PgGatewayMappingRepo::new(self.db.clone());
        repo.get_conversation_owner(conversation_id)
            .await
            .map_err(|e| GatewayError::ConnectionFailed(e.to_string()))
    }

    /// Resolves a Sõber user ID to a username by querying the database.
    ///
    /// Returns `None` if the user is not found or the query fails.
    pub async fn resolve_username(&self, user_id: &UserId) -> Option<String> {
        let row: Option<(String,)> = sqlx::query_as("SELECT username FROM users WHERE id = $1")
            .bind(user_id.as_uuid())
            .fetch_optional(&self.db)
            .await
            .ok()?;
        row.map(|r| r.0)
    }

    /// Returns the database pool.
    pub fn db(&self) -> &PgPool {
        &self.db
    }

    /// Returns the blob store for attachment retrieval.
    pub fn blob_store(&self) -> &Arc<BlobStore> {
        &self.blob_store
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
