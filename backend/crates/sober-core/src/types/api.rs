//! Standardized API response envelope types.
//!
//! All successful responses use [`ApiResponse`] to wrap data in a
//! `{ "data": ... }` envelope. Error responses use the envelope defined
//! in [`crate::error`].

use axum::Json;
use axum_core::response::{IntoResponse, Response};
use serde::Serialize;

/// Successful API response wrapper.
///
/// Serializes to `{ "data": T }` with HTTP 200 OK.
#[derive(Debug, Serialize)]
#[must_use]
pub struct ApiResponse<T: Serialize> {
    /// The response payload.
    pub data: T,
}

impl<T: Serialize> ApiResponse<T> {
    /// Wraps the given value in a success response.
    pub fn new(data: T) -> Self {
        Self { data }
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_core::body::Body;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn api_response_serializes_with_data_key() {
        let response = ApiResponse::new("hello").into_response();
        let response: http::Response<Body> = response.into();
        assert_eq!(response.status(), http::StatusCode::OK);

        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["data"], "hello");
    }
}
