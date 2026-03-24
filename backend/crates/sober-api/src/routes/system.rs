//! System status endpoints.

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::routing::get;
use sober_core::error::AppError;
use sober_core::types::{ApiResponse, UserRepo};
use sober_db::PgUserRepo;

use crate::state::AppState;

/// Returns the system routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/system/status", get(system_status))
}

/// `GET /api/v1/system/status` — returns whether the instance has been initialized.
///
/// Unauthenticated. Returns `{ "data": { "initialized": true|false } }`.
async fn system_status(
    State(state): State<Arc<AppState>>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let user_repo = PgUserRepo::new(state.db.clone());
    let initialized = user_repo.has_users().await?;
    Ok(ApiResponse::new(
        serde_json::json!({ "initialized": initialized }),
    ))
}
