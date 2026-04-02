//! Evolution management routes.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use sober_auth::RequireAdmin;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, AutonomyLevel, EvolutionEvent, EvolutionEventId, EvolutionStatus, EvolutionType,
};

use crate::services::evolution::{EvolutionConfigResponse, UpdateConfigInput};
use crate::state::AppState;

/// Returns the evolution management routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/evolution", get(list_events))
        .route("/evolution/config", get(get_config).patch(update_config))
        .route("/evolution/timeline", get(get_timeline))
        .route("/evolution/{id}", get(get_event).patch(update_event))
}

#[derive(serde::Deserialize)]
struct ListEventsParams {
    #[serde(rename = "type")]
    evolution_type: Option<EvolutionType>,
    status: Option<EvolutionStatus>,
}

async fn list_events(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Query(params): Query<ListEventsParams>,
) -> Result<ApiResponse<Vec<EvolutionEvent>>, AppError> {
    let events = state
        .evolution
        .list_events(params.evolution_type, params.status)
        .await?;
    Ok(ApiResponse::new(events))
}

async fn get_event(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<EvolutionEvent>, AppError> {
    let event = state
        .evolution
        .get_event(EvolutionEventId::from_uuid(id))
        .await?;
    Ok(ApiResponse::new(event))
}

#[derive(serde::Deserialize)]
struct UpdateEventRequest {
    status: EvolutionStatus,
}

async fn update_event(
    State(state): State<Arc<AppState>>,
    RequireAdmin(user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<UpdateEventRequest>,
) -> Result<ApiResponse<EvolutionEvent>, AppError> {
    let updated = state
        .evolution
        .update_event(EvolutionEventId::from_uuid(id), body.status, user.user_id)
        .await?;
    Ok(ApiResponse::new(updated))
}

async fn get_config(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
) -> Result<ApiResponse<EvolutionConfigResponse>, AppError> {
    let config = state.evolution.get_config().await?;
    Ok(ApiResponse::new(config))
}

#[derive(serde::Deserialize)]
struct UpdateConfigRequest {
    #[serde(default)]
    plugin_autonomy: Option<AutonomyLevel>,
    #[serde(default)]
    skill_autonomy: Option<AutonomyLevel>,
    #[serde(default)]
    instruction_autonomy: Option<AutonomyLevel>,
    #[serde(default)]
    automation_autonomy: Option<AutonomyLevel>,
}

async fn update_config(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Json(body): Json<UpdateConfigRequest>,
) -> Result<ApiResponse<EvolutionConfigResponse>, AppError> {
    let config = state
        .evolution
        .update_config(UpdateConfigInput {
            plugin_autonomy: body.plugin_autonomy,
            skill_autonomy: body.skill_autonomy,
            instruction_autonomy: body.instruction_autonomy,
            automation_autonomy: body.automation_autonomy,
        })
        .await?;
    Ok(ApiResponse::new(config))
}

#[derive(serde::Deserialize)]
struct TimelineParams {
    #[serde(rename = "type")]
    evolution_type: Option<EvolutionType>,
    status: Option<EvolutionStatus>,
    #[serde(default = "default_timeline_limit")]
    limit: i64,
}

fn default_timeline_limit() -> i64 {
    50
}

async fn get_timeline(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Query(params): Query<TimelineParams>,
) -> Result<ApiResponse<Vec<EvolutionEvent>>, AppError> {
    let events = state
        .evolution
        .get_timeline(params.limit, params.evolution_type, params.status)
        .await?;
    Ok(ApiResponse::new(events))
}
