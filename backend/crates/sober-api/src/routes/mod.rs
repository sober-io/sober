//! Route handlers and router assembly.

pub mod auth;
pub mod conversations;
pub mod health;
pub mod mcp;
pub mod ws;

use std::sync::Arc;

use axum::Router;
use sober_auth::AuthLayer;
use sober_db::{PgRoleRepo, PgSessionRepo, PgUserRepo};

use crate::state::AppState;

/// Builds the complete API router with all routes and middleware.
pub fn build_router(state: Arc<AppState>) -> Router {
    let auth_layer = AuthLayer::<PgUserRepo, PgSessionRepo, PgRoleRepo>::new(state.auth.clone());

    let api = Router::new()
        .merge(health::routes())
        .merge(auth::routes())
        .merge(conversations::routes())
        .merge(mcp::routes())
        .merge(ws::routes())
        .layer(auth_layer)
        .with_state(state);

    Router::new().nest("/api/v1", api)
}
