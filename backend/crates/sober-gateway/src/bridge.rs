//! Platform bridge trait and registry.

use std::sync::Arc;

use dashmap::DashMap;
use sober_core::types::{PlatformId, PlatformType};

use crate::error::GatewayError;
use crate::types::{ExternalChannel, PlatformMessage};

/// Object-safe handle to a connected bridge, stored in the registry.
///
/// Each connected platform bridge implements this trait so it can be
/// stored in a `DashMap` and dispatched to polymorphically.
#[async_trait::async_trait]
pub trait PlatformBridgeHandle: Send + Sync {
    /// Sends a message to an external channel.
    async fn send_message(
        &self,
        channel_id: &str,
        content: PlatformMessage,
    ) -> Result<(), GatewayError>;

    /// Lists all channels visible to the bot on this platform.
    async fn list_channels(&self) -> Result<Vec<ExternalChannel>, GatewayError>;

    /// Returns the platform type this bridge connects to.
    fn platform_type(&self) -> PlatformType;
}

/// Registry of active platform bridge connections.
///
/// Stores one bridge handle per `PlatformId`. Bridges are inserted when a
/// platform connects and removed when it disconnects.
pub struct PlatformBridgeRegistry {
    bridges: DashMap<PlatformId, Arc<dyn PlatformBridgeHandle>>,
}

impl PlatformBridgeRegistry {
    /// Creates a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bridges: DashMap::new(),
        }
    }

    /// Inserts or replaces a bridge for the given platform.
    pub fn insert(&self, platform_id: PlatformId, bridge: Arc<dyn PlatformBridgeHandle>) {
        self.bridges.insert(platform_id, bridge);
    }

    /// Removes the bridge for the given platform.
    pub fn remove(&self, platform_id: &PlatformId) {
        self.bridges.remove(platform_id);
    }

    /// Returns the bridge for the given platform, if connected.
    pub fn get(&self, platform_id: &PlatformId) -> Option<Arc<dyn PlatformBridgeHandle>> {
        self.bridges.get(platform_id).map(|v| v.value().clone())
    }

    /// Returns the status of all connected platforms as `(platform_id, platform_type)` pairs.
    pub fn statuses(&self) -> Vec<(PlatformId, PlatformType)> {
        self.bridges
            .iter()
            .map(|entry| (*entry.key(), entry.value().platform_type()))
            .collect()
    }
}

impl Default for PlatformBridgeRegistry {
    fn default() -> Self {
        Self::new()
    }
}
