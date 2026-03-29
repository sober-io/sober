//! Workspace routes (currently empty — settings moved to conversation settings).

use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

/// Returns the workspace routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
}
