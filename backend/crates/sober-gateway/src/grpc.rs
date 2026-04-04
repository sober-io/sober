//! gRPC service implementation for the gateway.

use std::sync::Arc;

use sober_core::types::PlatformId;
use tonic::{Request, Response, Status};
use tracing::error;
use uuid::Uuid;

use crate::proto::{
    ExternalChannel, HealthRequest, HealthResponse, ListChannelsRequest, ListChannelsResponse,
    PlatformStatus, ReloadRequest, ReloadResponse, StatusRequest, StatusResponse,
    gateway_service_server::GatewayService,
};
use crate::service::GatewayService as GatewayServiceImpl;

/// gRPC service for the gateway admin interface.
pub struct GatewayGrpcService {
    service: Arc<GatewayServiceImpl>,
}

impl GatewayGrpcService {
    /// Creates a new gRPC service backed by the gateway service.
    pub fn new(service: Arc<GatewayServiceImpl>) -> Self {
        Self { service }
    }
}

#[tonic::async_trait]
impl GatewayService for GatewayGrpcService {
    async fn list_channels(
        &self,
        request: Request<ListChannelsRequest>,
    ) -> Result<Response<ListChannelsResponse>, Status> {
        let platform_id_str = request.into_inner().platform_id;

        let uuid = Uuid::parse_str(&platform_id_str)
            .map_err(|e| Status::invalid_argument(format!("invalid platform_id: {e}")))?;
        let platform_id = PlatformId::from_uuid(uuid);

        let bridge = self
            .service
            .bridge_registry()
            .get(&platform_id)
            .ok_or_else(|| {
                Status::not_found(format!("platform {platform_id_str} is not connected"))
            })?;

        let channels = bridge.list_channels().await.map_err(|e| {
            error!(error = %e, "list_channels failed");
            Status::internal(e.to_string())
        })?;

        let proto_channels: Vec<ExternalChannel> = channels
            .into_iter()
            .map(|ch| ExternalChannel {
                id: ch.id,
                name: ch.name,
                kind: ch.kind,
            })
            .collect();

        Ok(Response::new(ListChannelsResponse {
            channels: proto_channels,
        }))
    }

    async fn reload(
        &self,
        _request: Request<ReloadRequest>,
    ) -> Result<Response<ReloadResponse>, Status> {
        self.service.reload().await.map_err(|e| {
            error!(error = %e, "cache reload failed");
            Status::internal(e.to_string())
        })?;

        Ok(Response::new(ReloadResponse {}))
    }

    async fn status(
        &self,
        _request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let statuses = self.service.bridge_registry().statuses();

        let platforms: Vec<PlatformStatus> = statuses
            .into_iter()
            .map(|(platform_id, platform_type)| PlatformStatus {
                platform_id: platform_id.to_string(),
                platform_type: platform_type.to_string(),
                display_name: String::new(),
                status: "connected".to_string(),
                mapping_count: 0,
            })
            .collect();

        Ok(Response::new(StatusResponse { platforms }))
    }

    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse { healthy: true }))
    }
}
