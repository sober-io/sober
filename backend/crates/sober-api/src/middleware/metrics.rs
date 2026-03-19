//! HTTP request metrics middleware.
//!
//! Records request count, latency, and in-flight gauge for every HTTP
//! request passing through the API gateway.

use std::task::{Context, Poll};
use std::time::Instant;

use axum::extract::MatchedPath;
use axum_core::body::Body;
use http::{Request, Response};
use tower::{Layer, Service};

/// Tower [`Layer`] that records HTTP request metrics.
///
/// Emits:
/// - `sober_api_request_total` (counter) — method, path, status
/// - `sober_api_request_duration_seconds` (histogram) — method, path, status
/// - `sober_api_requests_in_flight` (gauge) — incremented on entry, decremented on exit
#[derive(Clone, Default)]
pub struct HttpMetricsLayer;

impl HttpMetricsLayer {
    /// Creates a new HTTP metrics layer.
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for HttpMetricsLayer {
    type Service = HttpMetricsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HttpMetricsService { inner }
    }
}

/// Middleware service that wraps each request to record metrics.
#[derive(Clone)]
pub struct HttpMetricsService<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for HttpMetricsService<S>
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
        let method = req.method().as_str().to_owned();
        let path = req
            .extensions()
            .get::<MatchedPath>()
            .map(|mp| mp.as_str().to_owned())
            .unwrap_or_else(|| "unmatched".to_owned());

        metrics::gauge!("sober_api_requests_in_flight").increment(1);
        let start = Instant::now();

        let mut inner = self.inner.clone();
        Box::pin(async move {
            let result = inner.call(req).await;

            let elapsed = start.elapsed().as_secs_f64();
            metrics::gauge!("sober_api_requests_in_flight").decrement(1);

            let status = match &result {
                Ok(resp) => resp.status().as_u16().to_string(),
                Err(_) => "500".to_owned(),
            };

            metrics::counter!(
                "sober_api_request_total",
                "method" => method.clone(),
                "path" => path.clone(),
                "status" => status.clone(),
            )
            .increment(1);

            metrics::histogram!(
                "sober_api_request_duration_seconds",
                "method" => method,
                "path" => path,
                "status" => status,
            )
            .record(elapsed);

            result
        })
    }
}
