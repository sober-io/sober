//! Skill listing route — proxies to agent gRPC service.

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::routing::get;
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::ApiResponse;

use crate::proto;
use crate::state::AppState;

/// Returns the skills routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/skills", get(list_skills))
}

#[derive(serde::Deserialize)]
struct ListSkillsParams {
    conversation_id: Option<String>,
}

/// `GET /api/v1/skills` — list available skills.
///
/// Proxies to the agent's `ListSkills` gRPC RPC. Returns skill names and
/// descriptions for frontend slash command registration.
async fn list_skills(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    axum::extract::Query(params): axum::extract::Query<ListSkillsParams>,
) -> Result<ApiResponse<Vec<serde_json::Value>>, AppError> {
    let mut client = state.agent_client.clone();

    let response = client
        .list_skills(proto::ListSkillsRequest {
            user_id: auth_user.user_id.to_string(),
            conversation_id: params.conversation_id,
        })
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let skills: Vec<serde_json::Value> = response
        .into_inner()
        .skills
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "description": s.description,
            })
        })
        .collect();

    Ok(ApiResponse::new(skills))
}
