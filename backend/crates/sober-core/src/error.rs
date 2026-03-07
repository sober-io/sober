//! Centralized error types for the Sober system.
//!
//! [`AppError`] is the primary error type used across all crates. It maps
//! directly to HTTP status codes via its [`axum_core::response::IntoResponse`]
//! implementation.

use axum::Json;
use axum_core::response::{IntoResponse, Response};
use http::StatusCode;
use serde::Serialize;

/// Application-wide error type.
///
/// Each variant maps to a specific HTTP status code. Use `?` with `From`
/// implementations to propagate errors ergonomically.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Resource not found (404).
    #[error("Not found: {0}")]
    NotFound(String),

    /// Invalid input or constraint violation (400).
    #[error("Validation error: {0}")]
    Validation(String),

    /// Missing or invalid credentials (401).
    #[error("Unauthorized")]
    Unauthorized,

    /// Insufficient permissions (403).
    #[error("Forbidden")]
    Forbidden,

    /// Duplicate or conflicting state (409).
    #[error("Conflict: {0}")]
    Conflict(String),

    /// Unexpected internal error (500). Wraps an opaque error for
    /// propagation at service boundaries.
    #[error("{0}")]
    Internal(Box<dyn std::error::Error + Send + Sync>),
}

/// JSON body for error responses.
#[derive(Debug, Serialize)]
struct ApiErrorEnvelope {
    error: ApiErrorBody,
}

/// Inner error object within the response envelope.
#[derive(Debug, Clone, Serialize)]
pub struct ApiErrorBody {
    /// Machine-readable error code (e.g. `"not_found"`).
    pub code: String,
    /// Human-readable error message.
    pub message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_type) = match &self {
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Validation(_) => (StatusCode::BAD_REQUEST, "validation_error"),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };

        let body = ApiErrorEnvelope {
            error: ApiErrorBody {
                code: error_type.to_owned(),
                message: self.to_string(),
            },
        };

        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_core::body::Body;
    use http::Response as HttpResponse;
    use http_body_util::BodyExt;

    async fn response_status_and_body(error: AppError) -> (StatusCode, serde_json::Value) {
        let response: HttpResponse<Body> = error.into_response().into();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, body)
    }

    #[tokio::test]
    async fn not_found_maps_to_404() {
        let (status, body) = response_status_and_body(AppError::NotFound("user 123".into())).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
        assert_eq!(body["error"]["message"], "Not found: user 123");
    }

    #[tokio::test]
    async fn validation_maps_to_400() {
        let (status, body) =
            response_status_and_body(AppError::Validation("email invalid".into())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "validation_error");
    }

    #[tokio::test]
    async fn unauthorized_maps_to_401() {
        let (status, body) = response_status_and_body(AppError::Unauthorized).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "unauthorized");
    }

    #[tokio::test]
    async fn forbidden_maps_to_403() {
        let (status, body) = response_status_and_body(AppError::Forbidden).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "forbidden");
    }

    #[tokio::test]
    async fn conflict_maps_to_409() {
        let (status, body) =
            response_status_and_body(AppError::Conflict("duplicate email".into())).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "conflict");
    }

    #[tokio::test]
    async fn internal_maps_to_500() {
        let (status, body) =
            response_status_and_body(AppError::Internal("something broke".into())).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["code"], "internal_error");
    }
}
