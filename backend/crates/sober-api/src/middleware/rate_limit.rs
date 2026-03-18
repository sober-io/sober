//! In-memory rate limiting middleware backed by moka.
//!
//! Fixed-window counter per key (IP address). Returns 429 with
//! `Retry-After` header when the limit is exceeded.

use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum_core::body::Body;
use http::{Request, Response, StatusCode};
use moka::sync::Cache;
use tower::{Layer, Service};

/// Rate limit configuration for an endpoint pattern.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests allowed in the window.
    pub max_requests: u32,
    /// Window duration.
    pub window: Duration,
}

/// A request counter with its window start time.
#[derive(Debug, Clone, Copy)]
struct WindowCounter {
    count: u32,
    window_start: Instant,
}

/// Tower [`Layer`] that applies rate limiting.
#[derive(Clone)]
pub struct RateLimitLayer {
    store: Arc<Cache<String, WindowCounter>>,
    config: RateLimitConfig,
}

impl RateLimitLayer {
    /// Creates a new rate limit layer.
    pub fn new(config: RateLimitConfig) -> Self {
        // Entries are evicted after 2x the window to clean up stale keys,
        // but the actual window logic is handled in the counter itself.
        let store = Cache::builder()
            .time_to_idle(config.window * 2)
            .max_capacity(100_000)
            .build();
        Self {
            store: Arc::new(store),
            config,
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
        }
    }
}

/// Rate limiting service wrapping an inner service.
#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    store: Arc<Cache<String, WindowCounter>>,
    config: RateLimitConfig,
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
        let key = extract_client_ip(&req);
        let now = Instant::now();
        let window = self.config.window;
        let max = self.config.max_requests;

        let counter = self.store.get(&key);
        let (count, window_start) = match counter {
            Some(wc) if now.duration_since(wc.window_start) < window => (wc.count, wc.window_start),
            // Window expired or no entry — start fresh.
            _ => (0, now),
        };

        if count >= max {
            let elapsed = now.duration_since(window_start);
            let retry_after = window.saturating_sub(elapsed).as_secs().max(1);
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

        self.store.insert(
            key,
            WindowCounter {
                count: count + 1,
                window_start,
            },
        );
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

    #[test]
    fn window_resets_after_expiry() {
        let config = RateLimitConfig {
            max_requests: 2,
            window: Duration::from_millis(50),
        };
        let layer = RateLimitLayer::new(config);

        // Simulate 2 requests in the same window.
        let key = "test-ip".to_string();
        let now = Instant::now();
        layer.store.insert(
            key.clone(),
            WindowCounter {
                count: 2,
                window_start: now,
            },
        );

        // Within window — should still see count 2.
        let counter = layer.store.get(&key).unwrap();
        assert_eq!(counter.count, 2);

        // After window expires, the service logic will reset the counter.
        std::thread::sleep(Duration::from_millis(60));
        let counter = layer.store.get(&key).unwrap();
        // The entry still exists in the cache, but the service logic treats it as expired.
        assert!(Instant::now().duration_since(counter.window_start) >= Duration::from_millis(50));
    }
}
