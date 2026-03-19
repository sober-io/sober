//! Observability setup: tracing, OpenTelemetry, and Prometheus metrics.
//!
//! Call [`init_telemetry`] once at application startup to configure the full
//! observability stack. All backends (Tempo, Prometheus) are optional —
//! the application functions normally without them.

use std::net::SocketAddr;

use axum::response::IntoResponse;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::trace::SdkTracerProvider;
use tokio::net::TcpListener;
use tracing::Subscriber;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::Environment;

/// Standard metric label constants.
pub mod labels {
    /// Service name label.
    pub const SERVICE: &str = "service";
    /// HTTP method label.
    pub const METHOD: &str = "method";
    /// HTTP status code label.
    pub const STATUS: &str = "status";
    /// Source crate label.
    pub const CRATE: &str = "crate";
}

/// Handle returned by [`init_telemetry`] that owns the observability resources.
///
/// Holds the Prometheus metrics handle and, if configured, the OpenTelemetry
/// trace provider. Dropping this guard shuts down the OTel provider gracefully,
/// flushing any buffered spans.
///
/// Store this in your application's main scope so it lives for the process
/// lifetime.
pub struct TelemetryGuard {
    /// Prometheus metrics handle for the `/metrics` endpoint.
    pub prometheus: PrometheusHandle,
    /// OpenTelemetry trace provider (present when OTLP export is configured).
    otel_provider: Option<SdkTracerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.otel_provider.take()
            && let Err(e) = provider.shutdown()
        {
            eprintln!("OpenTelemetry shutdown error: {e}");
        }
    }
}

/// Initializes the full observability stack.
///
/// Sets up:
/// 1. **Tracing subscriber** — pretty (dev) or JSON (prod) log output
/// 2. **OpenTelemetry traces** — exports to Tempo via OTLP (if `OTEL_EXPORTER_OTLP_ENDPOINT` is set)
/// 3. **Prometheus metrics** — always active, available via the returned guard
///
/// `default_filter` is used when `RUST_LOG` is not set (e.g.
/// `"sober_api=debug,tower_http=debug,info"`).
///
/// # Panics
///
/// Panics if the tracing subscriber cannot be initialized (e.g. called twice).
#[must_use]
pub fn init_telemetry(environment: Environment, default_filter: &str) -> TelemetryGuard {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));

    // Register W3C TraceContext propagator for cross-service trace correlation
    opentelemetry::global::set_text_map_propagator(
        opentelemetry_sdk::propagation::TraceContextPropagator::new(),
    );

    // Prometheus metrics — always active.
    // Default buckets cover typical HTTP/gRPC latencies. Per-metric overrides
    // can be added via set_buckets_for_metric if needed.
    let default_duration_buckets = &[
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
    ];
    let prometheus_handle = PrometheusBuilder::new()
        .set_buckets(default_duration_buckets)
        .expect("valid default histogram buckets")
        .install_recorder()
        .expect("failed to install Prometheus recorder");

    // OpenTelemetry trace layer (optional, based on env var)
    let (otel_layer, otel_provider) = try_init_otel_tracing();

    // Build the subscriber
    match environment {
        Environment::Development => {
            let fmt_layer = tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_ansi(true);

            tracing_subscriber::registry()
                .with(env_filter)
                .with(otel_layer)
                .with(fmt_layer)
                .init();
        }
        Environment::Production => {
            let fmt_layer = tracing_subscriber::fmt::layer()
                .json()
                .with_target(true)
                .flatten_event(true);

            tracing_subscriber::registry()
                .with(env_filter)
                .with(otel_layer)
                .with(fmt_layer)
                .init();
        }
    }

    TelemetryGuard {
        prometheus: prometheus_handle,
        otel_provider,
    }
}

/// Attempts to initialize OpenTelemetry tracing.
///
/// Returns `(Some(layer), Some(provider))` if `OTEL_EXPORTER_OTLP_ENDPOINT` is set,
/// `(None, None)` otherwise. The caller must keep the provider alive for the
/// duration of the application.
fn try_init_otel_tracing<S>() -> (
    Option<tracing_opentelemetry::OpenTelemetryLayer<S, opentelemetry_sdk::trace::SdkTracer>>,
    Option<SdkTracerProvider>,
)
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    let endpoint = match std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        Ok(e) => e,
        Err(_) => return (None, None),
    };

    let exporter = match SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()
    {
        Ok(e) => {
            eprintln!("OTEL: OTLP exporter configured for {endpoint}");
            e
        }
        Err(e) => {
            eprintln!("WARNING: failed to create OTLP exporter: {e}");
            return (None, None);
        }
    };

    let service_name = std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "sober".to_owned());

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name(service_name)
                .build(),
        )
        .build();

    let tracer = provider.tracer("sober");
    let layer = tracing_opentelemetry::layer().with_tracer(tracer);

    (Some(layer), Some(provider))
}

/// Axum handler that renders Prometheus metrics in the text exposition format.
///
/// Mount this at `/metrics` in your router:
///
/// ```rust,ignore
/// let guard = init_telemetry(environment, "info");
/// let app = Router::new()
///     .route("/metrics", get(MetricsEndpoint(guard.prometheus.clone())));
/// ```
#[derive(Clone)]
pub struct MetricsEndpoint(pub PrometheusHandle);

impl IntoResponse for MetricsEndpoint {
    fn into_response(self) -> axum_core::response::Response {
        self.0.render().into_response()
    }
}

/// Spawns a lightweight HTTP server that serves only the `/metrics` endpoint.
///
/// Useful for gRPC-only binaries (agent, scheduler) that need an HTTP endpoint
/// for Prometheus scraping but don't run an axum-based HTTP server.
///
/// # Panics
///
/// Panics if the TCP listener cannot bind to the given port.
pub fn spawn_metrics_server(handle: PrometheusHandle, port: u16) {
    tokio::spawn(async move {
        let app =
            axum::Router::new().route("/metrics", axum::routing::get(MetricsEndpoint(handle)));
        let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port)))
            .await
            .expect("metrics server bind failed");
        tracing::info!(port, "metrics server listening");
        axum::serve(listener, app).await.ok();
    });
}

// ---------------------------------------------------------------------------
// gRPC trace propagation (feature-gated behind `grpc-telemetry`)
// ---------------------------------------------------------------------------

/// Wrapper around tonic `MetadataMap` implementing the OpenTelemetry `Injector`
/// trait, allowing trace context to be injected into outgoing gRPC requests.
#[cfg(feature = "grpc-telemetry")]
pub struct MetadataMapInjector<'a>(pub &'a mut tonic::metadata::MetadataMap);

#[cfg(feature = "grpc-telemetry")]
impl opentelemetry::propagation::Injector for MetadataMapInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let Ok(key) = tonic::metadata::MetadataKey::from_bytes(key.as_bytes())
            && let Ok(val) = tonic::metadata::MetadataValue::try_from(&value)
        {
            self.0.insert(key, val);
        }
    }
}

/// Wrapper around tonic `MetadataMap` implementing the OpenTelemetry `Extractor`
/// trait, allowing trace context to be extracted from incoming gRPC requests.
#[cfg(feature = "grpc-telemetry")]
pub struct MetadataMapExtractor<'a>(pub &'a tonic::metadata::MetadataMap);

#[cfg(feature = "grpc-telemetry")]
impl opentelemetry::propagation::Extractor for MetadataMapExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|val| val.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0
            .keys()
            .filter_map(|key| match key {
                tonic::metadata::KeyRef::Ascii(k) => Some(k.as_str()),
                tonic::metadata::KeyRef::Binary(_) => None,
            })
            .collect()
    }
}

/// Injects the current span's trace context into a tonic `MetadataMap`.
///
/// Call this on the client side before sending a gRPC request to propagate
/// trace context to the downstream service.
#[cfg(feature = "grpc-telemetry")]
pub fn inject_trace_context(metadata: &mut tonic::metadata::MetadataMap) {
    opentelemetry::global::get_text_map_propagator(|p| {
        p.inject(&mut MetadataMapInjector(metadata));
    });
}

/// Extracts trace context from a tonic `MetadataMap` and attaches it to the
/// current OpenTelemetry context.
///
/// Call this on the server side when receiving a gRPC request to link the
/// incoming span to the caller's trace.
#[cfg(feature = "grpc-telemetry")]
pub fn extract_trace_context(metadata: &tonic::metadata::MetadataMap) -> opentelemetry::Context {
    opentelemetry::global::get_text_map_propagator(|p| p.extract(&MetadataMapExtractor(metadata)))
}
