//! Evolution management routes.
//!
//! Admin-only endpoints for viewing, approving/rejecting, reverting, and
//! configuring self-evolution events. The update handler validates status
//! transitions and dispatches execution/revert to the agent via gRPC.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use sober_auth::RequireAdmin;
use sober_core::error::AppError;
use sober_core::types::{
    ApiResponse, AutonomyLevel, EvolutionConfigRow, EvolutionEvent, EvolutionEventId,
    EvolutionRepo, EvolutionStatus, EvolutionType,
};
use sober_db::PgEvolutionRepo;

use crate::proto;
use crate::state::AppState;

/// Returns the evolution management routes.
///
/// Routes with literal path segments (`/config`, `/timeline`) are registered
/// before the `/{id}` capture to prevent axum from matching them as IDs.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/evolution", get(list_events))
        .route("/evolution/config", get(get_config).patch(update_config))
        .route("/evolution/timeline", get(get_timeline))
        .route("/evolution/{id}", get(get_event).patch(update_event))
}

// ---------------------------------------------------------------------------
// Query / request types
// ---------------------------------------------------------------------------

/// Query parameters for `GET /api/v1/evolution`.
#[derive(serde::Deserialize)]
struct ListEventsParams {
    /// Filter by evolution type (plugin, skill, instruction, automation).
    #[serde(rename = "type")]
    evolution_type: Option<EvolutionType>,
    /// Filter by status (proposed, approved, executing, active, failed, rejected, reverted).
    status: Option<EvolutionStatus>,
}

/// Query parameters for `GET /api/v1/evolution/timeline`.
#[derive(serde::Deserialize)]
struct TimelineParams {
    /// Filter by evolution type.
    #[serde(rename = "type")]
    evolution_type: Option<EvolutionType>,
    /// Filter by status.
    status: Option<EvolutionStatus>,
    /// Maximum number of events to return (default 50).
    #[serde(default = "default_timeline_limit")]
    limit: i64,
}

fn default_timeline_limit() -> i64 {
    50
}

/// Request body for `PATCH /api/v1/evolution/{id}`.
#[derive(serde::Deserialize)]
struct UpdateEventRequest {
    /// Target status: `approved`, `rejected`, or `reverted`.
    status: EvolutionStatus,
}

/// Combined response for `GET /api/v1/evolution/config`.
#[derive(serde::Serialize)]
struct EvolutionConfigResponse {
    /// How often the evolution check job runs (e.g. "2h").
    interval: String,
    /// Autonomy level for plugin evolutions.
    plugin_autonomy: AutonomyLevel,
    /// Autonomy level for skill evolutions.
    skill_autonomy: AutonomyLevel,
    /// Autonomy level for instruction evolutions.
    instruction_autonomy: AutonomyLevel,
    /// Autonomy level for automation evolutions.
    automation_autonomy: AutonomyLevel,
}

/// Request body for `PATCH /api/v1/evolution/config`.
#[derive(serde::Deserialize)]
struct UpdateConfigRequest {
    /// Autonomy level for plugin evolutions.
    #[serde(default)]
    plugin_autonomy: Option<AutonomyLevel>,
    /// Autonomy level for skill evolutions.
    #[serde(default)]
    skill_autonomy: Option<AutonomyLevel>,
    /// Autonomy level for instruction evolutions.
    #[serde(default)]
    instruction_autonomy: Option<AutonomyLevel>,
    /// Autonomy level for automation evolutions.
    #[serde(default)]
    automation_autonomy: Option<AutonomyLevel>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/v1/evolution` — list evolution events with optional filters.
///
/// Query params: `type` (evolution type), `status` (lifecycle status).
/// Requires admin role.
async fn list_events(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Query(params): Query<ListEventsParams>,
) -> Result<ApiResponse<Vec<EvolutionEvent>>, AppError> {
    let repo = PgEvolutionRepo::new(state.db.clone());
    let events = repo.list(params.evolution_type, params.status).await?;
    Ok(ApiResponse::new(events))
}

/// `GET /api/v1/evolution/{id}` — get a single evolution event by ID.
///
/// Requires admin role. Returns 404 if the event does not exist.
async fn get_event(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<ApiResponse<EvolutionEvent>, AppError> {
    let repo = PgEvolutionRepo::new(state.db.clone());
    let event = repo.get_by_id(EvolutionEventId::from_uuid(id)).await?;
    Ok(ApiResponse::new(event))
}

/// `PATCH /api/v1/evolution/{id}` — update an evolution event's status.
///
/// Validates allowed transitions:
/// - `proposed` or `failed` -> `approved` (approve / retry)
/// - `proposed` -> `rejected`
/// - `active` -> `reverted`
///
/// For `approved`: updates DB then calls agent `ExecuteEvolution` gRPC.
/// For `reverted`: updates DB then calls agent `RevertEvolution` gRPC.
/// For `rejected`: updates DB only.
///
/// Requires admin role. The authenticated user is recorded as `decided_by`.
async fn update_event(
    State(state): State<Arc<AppState>>,
    RequireAdmin(user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<UpdateEventRequest>,
) -> Result<ApiResponse<EvolutionEvent>, AppError> {
    let repo = PgEvolutionRepo::new(state.db.clone());
    let event_id = EvolutionEventId::from_uuid(id);

    // Fetch current event to validate transition.
    let current = repo.get_by_id(event_id).await?;

    // Validate status transition.
    match (&current.status, &body.status) {
        (EvolutionStatus::Proposed | EvolutionStatus::Failed, EvolutionStatus::Approved) => {}
        (EvolutionStatus::Proposed, EvolutionStatus::Rejected) => {}
        (EvolutionStatus::Active, EvolutionStatus::Reverted) => {}
        _ => {
            return Err(AppError::Validation(format!(
                "cannot transition from {} to {}",
                serde_json::to_string(&current.status)
                    .unwrap_or_default()
                    .trim_matches('"'),
                serde_json::to_string(&body.status)
                    .unwrap_or_default()
                    .trim_matches('"'),
            )));
        }
    }

    // Update status in DB with the admin as decided_by.
    repo.update_status(event_id, body.status, Some(user.user_id))
        .await?;

    // Dispatch gRPC calls for statuses that trigger agent-side work.
    match body.status {
        EvolutionStatus::Approved => {
            let mut client = state.agent_client.clone();
            client
                .execute_evolution(proto::ExecuteEvolutionRequest {
                    evolution_event_id: id.to_string(),
                })
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        EvolutionStatus::Reverted => {
            let mut client = state.agent_client.clone();
            client
                .revert_evolution(proto::RevertEvolutionRequest {
                    evolution_event_id: id.to_string(),
                })
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        _ => {}
    }

    // Re-fetch to return updated state.
    let updated = repo.get_by_id(event_id).await?;
    Ok(ApiResponse::new(updated))
}

/// `GET /api/v1/evolution/config` — get evolution configuration.
///
/// Merges the DB-backed autonomy levels with the `interval` from `AppConfig`.
/// Requires admin role.
async fn get_config(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
) -> Result<ApiResponse<EvolutionConfigResponse>, AppError> {
    let repo = PgEvolutionRepo::new(state.db.clone());
    let config = repo.get_config().await?;

    Ok(ApiResponse::new(EvolutionConfigResponse {
        interval: state.config.evolution.interval.clone(),
        plugin_autonomy: config.plugin_autonomy,
        skill_autonomy: config.skill_autonomy,
        instruction_autonomy: config.instruction_autonomy,
        automation_autonomy: config.automation_autonomy,
    }))
}

/// `PATCH /api/v1/evolution/config` — update evolution autonomy configuration.
///
/// Accepts a partial update — only the provided fields are changed.
/// Requires admin role.
async fn update_config(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Json(body): Json<UpdateConfigRequest>,
) -> Result<ApiResponse<EvolutionConfigResponse>, AppError> {
    let repo = PgEvolutionRepo::new(state.db.clone());

    // Fetch current config to merge with partial update.
    let current = repo.get_config().await?;

    let updated_config = EvolutionConfigRow {
        plugin_autonomy: body.plugin_autonomy.unwrap_or(current.plugin_autonomy),
        skill_autonomy: body.skill_autonomy.unwrap_or(current.skill_autonomy),
        instruction_autonomy: body
            .instruction_autonomy
            .unwrap_or(current.instruction_autonomy),
        automation_autonomy: body
            .automation_autonomy
            .unwrap_or(current.automation_autonomy),
        updated_at: current.updated_at, // repo will set the actual timestamp
    };

    let saved = repo.update_config(updated_config).await?;

    Ok(ApiResponse::new(EvolutionConfigResponse {
        interval: state.config.evolution.interval.clone(),
        plugin_autonomy: saved.plugin_autonomy,
        skill_autonomy: saved.skill_autonomy,
        instruction_autonomy: saved.instruction_autonomy,
        automation_autonomy: saved.automation_autonomy,
    }))
}

/// `GET /api/v1/evolution/timeline` — list evolution events in timeline order.
///
/// Returns events ordered by `created_at DESC` for the timeline view.
/// Query params: `type`, `status`, `limit` (default 50).
/// Requires admin role.
async fn get_timeline(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Query(params): Query<TimelineParams>,
) -> Result<ApiResponse<Vec<EvolutionEvent>>, AppError> {
    let repo = PgEvolutionRepo::new(state.db.clone());
    let events = repo
        .list_timeline(params.limit, params.evolution_type, params.status)
        .await?;
    Ok(ApiResponse::new(events))
}
