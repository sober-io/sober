//! User route handlers.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, State};
use axum::routing::get;
use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::ApiResponse;

use crate::services::user::UserSearchResult;
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
async fn search_users(
    State(state): State<Arc<AppState>>,
    _auth_user: AuthUser,
    Query(params): Query<SearchUsersQuery>,
) -> Result<ApiResponse<Vec<UserSearchResult>>, AppError> {
    let results = state.user.search(&params.q, 10).await?;
    Ok(ApiResponse::new(results))
}
