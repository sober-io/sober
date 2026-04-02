//! Route handlers and router assembly.

pub mod attachments;
pub mod auth;
pub mod collaborators;
pub mod conversations;
pub mod evolution;
pub mod health;
pub mod messages;
pub mod plugins;
pub mod system;
pub mod tags;
pub mod users;
pub mod workspaces;
pub mod ws;

use std::sync::Arc;

use axum::Router;
use sober_auth::AuthLayer;
use sober_db::{PgRoleRepo, PgSessionRepo, PgUserRepo};

pub(crate) use crate::services::verify_membership;

use crate::state::AppState;

/// Builds the complete API router with all routes and middleware.
pub fn build_router(state: Arc<AppState>) -> Router {
    let auth_layer = AuthLayer::<PgUserRepo, PgSessionRepo, PgRoleRepo>::new(state.auth.clone());

    let api = Router::new()
        .merge(health::routes())
        .merge(system::routes())
        .merge(attachments::routes())
        .merge(auth::routes())
        .merge(conversations::routes())
        .merge(collaborators::routes())
        .merge(evolution::routes())
        .merge(messages::routes())
        .merge(plugins::routes())
        .merge(tags::routes())
        .merge(users::routes())
        .merge(workspaces::routes())
        .merge(ws::routes())
        .layer(auth_layer)
        .with_state(state);

    Router::new().nest("/api/v1", api)
}
