//! Health check endpoint.

use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use sober_core::types::ApiResponse;

use crate::state::AppState;

/// Returns the health check route.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/health", get(health_check))
}

/// `GET /api/v1/health` — returns server health status.
async fn health_check() -> ApiResponse<serde_json::Value> {
    ApiResponse::new(serde_json::json!({ "status": "ok" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_returns_ok() {
        let response = health_check().await;
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["data"]["status"], "ok");
    }
}
