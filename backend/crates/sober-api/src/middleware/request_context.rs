//! Records `user.id` and `request.id` on the current tracing span.

use std::task::{Context, Poll};

use axum_core::body::Body;
use http::Request;
use sober_auth::AuthUser;
use tower::{Layer, Service};
use tracing::Span;

/// Layer that records `user.id` and `request.id` on the current tracing span.
#[derive(Clone, Default)]
pub struct RequestContextLayer;

impl RequestContextLayer {
    /// Creates a new `RequestContextLayer`.
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for RequestContextLayer {
    type Service = RequestContextService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestContextService { inner }
    }
}

#[derive(Clone)]
pub struct RequestContextService<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for RequestContextService<S>
where
    S: Service<Request<Body>> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let span = Span::current();

        if let Some(auth_user) = req.extensions().get::<AuthUser>() {
            span.record("user.id", tracing::field::display(&auth_user.user_id));
        }

        if let Some(request_id) = req
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
        {
            span.record("request.id", request_id);
        }

        self.inner.call(req)
    }
}
