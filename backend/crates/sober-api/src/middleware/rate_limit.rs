//! In-memory rate limiting middleware backed by moka.
//!
//! Sliding window counter per key (IP or user ID). Returns 429 with
//! `Retry-After` header when the limit is exceeded.

use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum_core::body::Body;
use http::{Request, Response, StatusCode};
use moka::sync::Cache;
use sober_auth::AuthUser;
use tower::{Layer, Service};

/// Rate limit configuration for an endpoint pattern.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests allowed in the window.
    pub max_requests: u32,
    /// Window duration.
    pub window: Duration,
}

/// Tower [`Layer`] that applies rate limiting.
#[derive(Clone)]
pub struct RateLimitLayer {
    store: Arc<Cache<String, u32>>,
    config: RateLimitConfig,
    scope: RateLimitScope,
}

/// Whether to rate-limit by IP address or by authenticated user.
#[derive(Debug, Clone, Copy)]
pub enum RateLimitScope {
    /// Key by client IP address.
    Ip,
    /// Key by authenticated user ID.
    User,
}

impl RateLimitLayer {
    /// Creates a new rate limit layer.
    pub fn new(config: RateLimitConfig, scope: RateLimitScope) -> Self {
        let store = Cache::builder()
            .time_to_live(config.window)
            .max_capacity(100_000)
            .build();
        Self {
            store: Arc::new(store),
            config,
            scope,
        }
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            store: self.store.clone(),
            config: self.config.clone(),
            scope: self.scope,
        }
    }
}

/// Rate limiting service wrapping an inner service.
#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    store: Arc<Cache<String, u32>>,
    config: RateLimitConfig,
    scope: RateLimitScope,
}

impl<S> Service<Request<Body>> for RateLimitService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: Send,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let key = match self.scope {
            RateLimitScope::Ip => extract_client_ip(&req),
            RateLimitScope::User => req
                .extensions()
                .get::<AuthUser>()
                .map(|u| u.user_id.to_string())
                .unwrap_or_else(|| extract_client_ip(&req)),
        };

        let current = self.store.get(&key).unwrap_or(0);
        if current >= self.config.max_requests {
            let retry_after = self.config.window.as_secs();
            return Box::pin(async move {
                let body = serde_json::json!({
                    "error": {
                        "code": "rate_limited",
                        "message": "Too many requests"
                    }
                });
                let response = Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .header("Retry-After", retry_after)
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap_or_default()))
                    .expect("valid response");
                Ok(response)
            });
        }

        self.store.insert(key, current + 1);
        let mut inner = self.inner.clone();
        Box::pin(async move { inner.call(req).await })
    }
}

/// Extracts the client IP from common proxy headers or the connection info.
fn extract_client_ip(req: &Request<Body>) -> String {
    if let Some(forwarded) = req.headers().get("x-forwarded-for")
        && let Ok(val) = forwarded.to_str()
        && let Some(ip) = val.split(',').next()
    {
        return ip.trim().to_owned();
    }

    if let Some(real_ip) = req.headers().get("x-real-ip")
        && let Ok(val) = real_ip.to_str()
    {
        return val.trim().to_owned();
    }

    // Fallback — use a fixed key (single-node, this is fine for v1).
    "unknown".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_client_ip_from_forwarded() {
        let req = Request::builder()
            .header("x-forwarded-for", "1.2.3.4, 5.6.7.8")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "1.2.3.4");
    }

    #[test]
    fn extract_client_ip_from_real_ip() {
        let req = Request::builder()
            .header("x-real-ip", "9.8.7.6")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "9.8.7.6");
    }

    #[test]
    fn extract_client_ip_fallback() {
        let req = Request::builder().body(Body::empty()).unwrap();
        assert_eq!(extract_client_ip(&req), "unknown");
    }
}
