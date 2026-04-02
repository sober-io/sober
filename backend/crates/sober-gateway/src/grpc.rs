//! gRPC service implementation for the gateway.

use tonic::{Request, Response, Status};

use crate::proto::{
    ExternalChannel, HealthRequest, HealthResponse, ListChannelsRequest, ListChannelsResponse,
    ReloadRequest, ReloadResponse, StatusRequest, StatusResponse,
    gateway_service_server::GatewayService,
};

/// Stub gRPC service — full implementation added in Task 12.
#[derive(Debug, Default)]
pub struct GatewayGrpcService;

#[tonic::async_trait]
impl GatewayService for GatewayGrpcService {
    async fn list_channels(
        &self,
        _request: Request<ListChannelsRequest>,
    ) -> Result<Response<ListChannelsResponse>, Status> {
        Ok(Response::new(ListChannelsResponse {
            channels: Vec::<ExternalChannel>::new(),
        }))
    }

    async fn reload(
        &self,
        _request: Request<ReloadRequest>,
    ) -> Result<Response<ReloadResponse>, Status> {
        Ok(Response::new(ReloadResponse {}))
    }

    async fn status(
        &self,
        _request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        Ok(Response::new(StatusResponse { platforms: vec![] }))
    }

    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse { healthy: true }))
    }
}
