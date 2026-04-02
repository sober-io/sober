//! Gateway admin routes — platform, channel mapping, and user mapping CRUD.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::Serialize;
use sober_auth::RequireAdmin;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, CreateChannelMapping, CreatePlatform, CreateUserMapping, GatewayChannelMapping,
    GatewayPlatform, GatewayUserMapping, MappingId, PlatformId, UpdatePlatform, UserMappingId,
};

use crate::gateway_proto;
use crate::state::AppState;

/// Serializable representation of an external platform channel.
#[derive(Serialize)]
pub struct ExternalChannelResponse {
    pub id: String,
    pub name: String,
    pub kind: String,
}

impl From<gateway_proto::ExternalChannel> for ExternalChannelResponse {
    fn from(c: gateway_proto::ExternalChannel) -> Self {
        Self {
            id: c.id,
            name: c.name,
            kind: c.kind,
        }
    }
}

/// Returns the gateway admin routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/admin/gateway/platforms",
            get(list_platforms).post(create_platform),
        )
        .route(
            "/admin/gateway/platforms/{id}",
            get(get_platform)
                .patch(update_platform)
                .delete(delete_platform),
        )
        .route(
            "/admin/gateway/platforms/{id}/channels",
            get(list_available_channels),
        )
        .route(
            "/admin/gateway/platforms/{id}/mappings",
            get(list_mappings).post(create_mapping),
        )
        .route("/admin/gateway/mappings/{id}", delete(delete_mapping))
        .route(
            "/admin/gateway/platforms/{id}/users",
            get(list_user_mappings).post(create_user_mapping),
        )
        .route(
            "/admin/gateway/user-mappings/{id}",
            delete(delete_user_mapping),
        )
}

async fn list_platforms(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
) -> Result<ApiResponse<Vec<GatewayPlatform>>, AppError> {
    let platforms = state.gateway_admin.list_platforms().await?;
    Ok(ApiResponse::new(platforms))
}

async fn get_platform(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<GatewayPlatform>, AppError> {
    let platform = state
        .gateway_admin
        .get_platform(PlatformId::from_uuid(id))
        .await?;
    Ok(ApiResponse::new(platform))
}

async fn create_platform(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Json(input): Json<CreatePlatform>,
) -> Result<ApiResponse<GatewayPlatform>, AppError> {
    let platform = state.gateway_admin.create_platform(input).await?;
    Ok(ApiResponse::new(platform))
}

async fn update_platform(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<UpdatePlatform>,
) -> Result<ApiResponse<GatewayPlatform>, AppError> {
    let platform = state
        .gateway_admin
        .update_platform(PlatformId::from_uuid(id), input)
        .await?;
    Ok(ApiResponse::new(platform))
}

async fn delete_platform(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state
        .gateway_admin
        .delete_platform(PlatformId::from_uuid(id))
        .await?;
    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}

async fn list_available_channels(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<Vec<ExternalChannelResponse>>, AppError> {
    let mut client = state
        .gateway_client
        .clone()
        .ok_or_else(|| AppError::Internal("gateway service is not available".into()))?;

    let mut request = tonic::Request::new(gateway_proto::ListChannelsRequest {
        platform_id: id.to_string(),
    });
    sober_core::inject_trace_context(request.metadata_mut());

    let response = client
        .list_channels(request)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let channels = response
        .into_inner()
        .channels
        .into_iter()
        .map(ExternalChannelResponse::from)
        .collect();

    Ok(ApiResponse::new(channels))
}

async fn list_mappings(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<Vec<GatewayChannelMapping>>, AppError> {
    let mappings = state
        .gateway_admin
        .list_mappings(PlatformId::from_uuid(id))
        .await?;
    Ok(ApiResponse::new(mappings))
}

async fn create_mapping(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<CreateChannelMapping>,
) -> Result<ApiResponse<GatewayChannelMapping>, AppError> {
    let mapping = state
        .gateway_admin
        .create_mapping(PlatformId::from_uuid(id), input)
        .await?;
    Ok(ApiResponse::new(mapping))
}

async fn delete_mapping(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state
        .gateway_admin
        .delete_mapping(MappingId::from_uuid(id))
        .await?;
    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}

async fn list_user_mappings(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<Vec<GatewayUserMapping>>, AppError> {
    let mappings = state
        .gateway_admin
        .list_user_mappings(PlatformId::from_uuid(id))
        .await?;
    Ok(ApiResponse::new(mappings))
}

async fn create_user_mapping(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<CreateUserMapping>,
) -> Result<ApiResponse<GatewayUserMapping>, AppError> {
    let mapping = state
        .gateway_admin
        .create_user_mapping(PlatformId::from_uuid(id), input)
        .await?;
    Ok(ApiResponse::new(mapping))
}

async fn delete_user_mapping(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    state
        .gateway_admin
        .delete_user_mapping(UserMappingId::from_uuid(id))
        .await?;
    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}
