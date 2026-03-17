//! User route handlers.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, State};
use axum::routing::get;
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{ApiResponse, UserRepo};
use sober_db::PgUserRepo;

use crate::state::AppState;

/// Returns the users routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/users/search", get(search_users))
}

/// Query parameters for `GET /users/search`.
#[derive(serde::Deserialize)]
struct SearchUsersQuery {
    q: String,
}

/// `GET /api/v1/users/search?q=<query>` — search active users by username prefix.
///
/// Returns up to 10 users whose username starts with the query string.
/// Only accessible to authenticated users.
async fn search_users(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Query(params): Query<SearchUsersQuery>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let repo = PgUserRepo::new(state.db.clone());
    let users = repo.search_by_username(&params.q, 10).await?;
    let results: Vec<_> = users
        .iter()
        .map(|u| {
            serde_json::json!({
                "id": u.id.to_string(),
                "username": u.username,
            })
        })
        .collect();
    Ok(ApiResponse::new(serde_json::json!(results)))
}
