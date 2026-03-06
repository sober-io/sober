# Observability Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add comprehensive observability (metrics, distributed tracing, structured logging, auto-generated dashboards, alerting) to every Sober service.

**Architecture:** Telemetry init lives in `sober-core`. Each crate declares metrics in `metrics.toml`. A Rust tool generates Grafana dashboards from those files. Docker Compose runs Prometheus, Tempo, Loki, and Grafana. OTEL traces propagate across gRPC boundaries via tonic interceptors.

**Tech Stack:** `tracing`, `tracing-opentelemetry`, `opentelemetry-otlp`, `metrics`, `metrics-exporter-prometheus`, Prometheus, Grafana Tempo, Grafana Loki, Promtail, Grafana

**Depends on:** 002 (project skeleton), 003 (sober-core)

---

## Steps

### Task 1: Add Telemetry Dependencies to Workspace

**Files:**
- Modify: `backend/Cargo.toml` (workspace dependencies section)

**Step 1: Add new workspace dependencies**

Add to the `[workspace.dependencies]` section in `backend/Cargo.toml`:

```toml
# Telemetry - OTEL
opentelemetry = { version = "0.29", features = ["trace"] }
opentelemetry_sdk = { version = "0.29", features = ["rt-tokio"] }
opentelemetry-otlp = { version = "0.29", features = ["tonic"] }
tracing-opentelemetry = "0.30"

# Telemetry - Metrics
metrics = "0.24"
metrics-exporter-prometheus = "0.17"
```

Look up exact latest versions on crates.io before adding. The versions above are placeholders.

**Step 2: Add dependencies to sober-core's Cargo.toml**

Add to `backend/crates/sober-core/Cargo.toml`:

```toml
opentelemetry = { workspace = true }
opentelemetry_sdk = { workspace = true }
opentelemetry-otlp = { workspace = true }
tracing-opentelemetry = { workspace = true }
metrics = { workspace = true }
metrics-exporter-prometheus = { workspace = true }
```

**Step 3: Verify it compiles**

Run: `cd backend && cargo check -p sober-core`
Expected: Compiles with no errors.

**Step 4: Commit**

```bash
git add backend/Cargo.toml backend/crates/sober-core/Cargo.toml
git commit -m "feat(core): add telemetry dependencies (OTEL, metrics, prometheus)"
```

---

### Task 2: Add Telemetry Config to AppConfig

**Files:**
- Modify: `backend/crates/sober-core/src/config.rs`
- Modify: `backend/crates/sober-core/src/lib.rs` (if re-exports needed)

**Step 1: Write the failing test**

Add to the test module in `config.rs`:

```rust
#[test]
fn telemetry_config_defaults_when_unset() {
    // Clear any existing telemetry env vars
    std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    std::env::remove_var("OTEL_SERVICE_NAME");
    std::env::remove_var("OTEL_TRACES_SAMPLER");
    std::env::remove_var("METRICS_LISTEN_ADDR");

    let config = TelemetryConfig::from_env();
    assert!(config.otel_endpoint.is_none());
    assert_eq!(config.service_name, "sober");
    assert_eq!(config.traces_sampler, "always_on");
    assert!(config.metrics_listen_addr.is_none());
}

#[test]
fn telemetry_config_reads_env() {
    std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://tempo:4317");
    std::env::set_var("OTEL_SERVICE_NAME", "sober-api");
    std::env::set_var("OTEL_TRACES_SAMPLER", "parentbased_traceidratio");
    std::env::set_var("METRICS_LISTEN_ADDR", "0.0.0.0:9000");

    let config = TelemetryConfig::from_env();
    assert_eq!(config.otel_endpoint.as_deref(), Some("http://tempo:4317"));
    assert_eq!(config.service_name, "sober-api");
    assert_eq!(config.traces_sampler, "parentbased_traceidratio");
    assert_eq!(config.metrics_listen_addr.as_deref(), Some("0.0.0.0:9000"));

    // Cleanup
    std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    std::env::remove_var("OTEL_SERVICE_NAME");
    std::env::remove_var("OTEL_TRACES_SAMPLER");
    std::env::remove_var("METRICS_LISTEN_ADDR");
}
```

**Step 2: Run test to verify it fails**

Run: `cd backend && cargo test -p sober-core -- telemetry_config`
Expected: FAIL — `TelemetryConfig` not found.

**Step 3: Implement TelemetryConfig**

Add to `config.rs`:

```rust
/// Configuration for telemetry (OTEL traces, Prometheus metrics).
///
/// All fields have sensible defaults. OTEL export is disabled when
/// `otel_endpoint` is `None`.
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// OTLP endpoint for trace export (e.g., `http://tempo:4317`).
    /// When `None`, OTEL trace export is disabled.
    pub otel_endpoint: Option<String>,
    /// Service name for OTEL traces. Defaults to `"sober"`.
    pub service_name: String,
    /// OTEL trace sampler. Defaults to `"always_on"`.
    pub traces_sampler: String,
    /// Bind address for the `/metrics` Prometheus endpoint.
    /// When `None`, metrics are served on the main service port.
    pub metrics_listen_addr: Option<String>,
}

impl TelemetryConfig {
    /// Load telemetry config from environment variables.
    ///
    /// All variables are optional with sensible defaults.
    pub fn from_env() -> Self {
        Self {
            otel_endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok(),
            service_name: std::env::var("OTEL_SERVICE_NAME")
                .unwrap_or_else(|_| "sober".to_owned()),
            traces_sampler: std::env::var("OTEL_TRACES_SAMPLER")
                .unwrap_or_else(|_| "always_on".to_owned()),
            metrics_listen_addr: std::env::var("METRICS_LISTEN_ADDR").ok(),
        }
    }
}
```

Add a `pub telemetry: TelemetryConfig` field to `AppConfig` and call `TelemetryConfig::from_env()` in `AppConfig::load_from_env()`.

**Step 4: Run test to verify it passes**

Run: `cd backend && cargo test -p sober-core -- telemetry_config`
Expected: PASS

**Step 5: Commit**

```bash
git add backend/crates/sober-core/src/config.rs
git commit -m "feat(core): add TelemetryConfig for OTEL and metrics settings"
```

---

### Task 3: Verify init_telemetry() from plan 003

> **Note:** `init_telemetry()` is implemented as part of plan 003 (sober-core).
> This task only verifies it works correctly with OTEL and Prometheus backends,
> and extends it if needed for gRPC trace propagation (Task 5).

**Verification test:**

```rust
#[test]
fn init_telemetry_without_otel_does_not_panic() {
    let config = TelemetryConfig {
        otel_endpoint: None,
        service_name: "test".to_owned(),
        traces_sampler: "always_on".to_owned(),
        metrics_listen_addr: None,
    };
    // Should not panic — OTEL disabled, only tracing subscriber + metrics recorder
    let _guard = init_telemetry(&config);
}
```

Run: `cd backend && cargo test -p sober-core -- init_telemetry`
Expected: PASS (already implemented in plan 003).

If this fails, the implementation from plan 003 needs to be updated.
The reference implementation below captures the intent:

```rust
use crate::config::TelemetryConfig;
use metrics_exporter_prometheus::PrometheusBuilder;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Guard that shuts down the OTEL tracer provider on drop.
pub struct TelemetryGuard {
    _prometheus_handle: metrics_exporter_prometheus::PrometheusHandle,
}

impl TelemetryGuard {
    /// Returns the Prometheus handle for serving `/metrics`.
    pub fn prometheus_handle(&self) -> &metrics_exporter_prometheus::PrometheusHandle {
        &self._prometheus_handle
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        opentelemetry::global::shutdown_tracer_provider();
    }
}

/// Initialize the telemetry stack: tracing subscriber, optional OTEL export,
/// and Prometheus metrics recorder.
///
/// Returns a guard that must be held for the lifetime of the application.
/// Dropping the guard shuts down the OTEL tracer provider.
pub fn init_telemetry(config: &TelemetryConfig) -> TelemetryGuard {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let is_dev = std::env::var("SOBER_ENV")
        .map(|v| v == "development")
        .unwrap_or(true);

    // Prometheus metrics recorder
    let prometheus_handle = PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install Prometheus metrics recorder");

    // Build subscriber layers
    let registry = tracing_subscriber::registry().with(env_filter);

    if let Some(endpoint) = &config.otel_endpoint {
        // OTEL trace exporter
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()
            .expect("failed to build OTLP span exporter");

        let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(
                opentelemetry_sdk::Resource::builder()
                    .with_service_name(config.service_name.clone())
                    .build(),
            )
            .build();

        let tracer = tracer_provider.tracer(config.service_name.clone());
        opentelemetry::global::set_tracer_provider(tracer_provider);

        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        if is_dev {
            registry
                .with(fmt::layer().pretty())
                .with(otel_layer)
                .init();
        } else {
            registry
                .with(fmt::layer().json())
                .with(otel_layer)
                .init();
        }
    } else {
        // No OTEL — tracing only
        if is_dev {
            registry.with(fmt::layer().pretty()).init();
        } else {
            registry.with(fmt::layer().json()).init();
        }
    }

    TelemetryGuard {
        _prometheus_handle: prometheus_handle,
    }
}
```

Note: The exact OTEL API may differ — check the `opentelemetry` 0.29+ docs before implementing. The above captures the intent; the builder API evolves between versions. Use context7 MCP or crates.io docs to confirm exact method names.

**Verification:** `lib.rs` already exports `pub use telemetry::init_telemetry` (from plan 003).
No backward compatibility shim needed — `init_tracing` never existed in code.

---

### Task 4: Add Prometheus /metrics Endpoint Handler

**Files:**
- Create: `backend/crates/sober-core/src/metrics_endpoint.rs`
- Modify: `backend/crates/sober-core/src/lib.rs`

**Step 1: Write the failing test**

```rust
#[tokio::test]
async fn metrics_endpoint_returns_200() {
    use axum::{body::Body, http::Request, routing::get, Router};
    use tower::ServiceExt;

    let config = TelemetryConfig {
        otel_endpoint: None,
        service_name: "test".to_owned(),
        traces_sampler: "always_on".to_owned(),
        metrics_listen_addr: None,
    };
    let guard = init_telemetry(&config);
    let handle = guard.prometheus_handle().clone();

    let app = Router::new().route("/metrics", get(metrics_handler));
    let app = app.with_state(handle);

    let response = app
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
}
```

**Step 2: Run test to verify it fails**

Run: `cd backend && cargo test -p sober-core -- metrics_endpoint`
Expected: FAIL — `metrics_handler` not found.

**Step 3: Implement the handler**

```rust
use axum::{extract::State, response::IntoResponse};
use metrics_exporter_prometheus::PrometheusHandle;

/// Axum handler that serves Prometheus metrics.
///
/// Wire this into your router as:
/// ```rust
/// Router::new()
///     .route("/metrics", get(sober_core::metrics_handler))
///     .with_state(prometheus_handle);
/// ```
pub async fn metrics_handler(
    State(handle): State<PrometheusHandle>,
) -> impl IntoResponse {
    handle.render()
}
```

**Step 4: Re-export from lib.rs**

```rust
pub use metrics_endpoint::metrics_handler;
```

**Step 5: Run test to verify it passes**

Run: `cd backend && cargo test -p sober-core -- metrics_endpoint`
Expected: PASS

**Step 6: Commit**

```bash
git add backend/crates/sober-core/src/metrics_endpoint.rs backend/crates/sober-core/src/lib.rs
git commit -m "feat(core): add Prometheus /metrics axum handler"
```

---

### Task 5: Add gRPC Trace Propagation Helpers

**Files:**
- Create: `backend/crates/sober-core/src/grpc_telemetry.rs`
- Modify: `backend/crates/sober-core/src/lib.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn trace_context_inject_extract_roundtrip() {
    use tonic::metadata::MetadataMap;

    // Create a span and inject its context into metadata
    let span = tracing::info_span!("test_span");
    let _guard = span.enter();

    let mut metadata = MetadataMap::new();
    inject_trace_context(&mut metadata);

    // Extract should produce a valid context
    // (In a real test with OTEL initialized, this would carry the trace ID)
    extract_trace_context(&metadata);
}
```

**Step 2: Run test to verify it fails**

Run: `cd backend && cargo test -p sober-core -- trace_context`
Expected: FAIL — functions not found.

**Step 3: Implement trace propagation helpers**

```rust
use opentelemetry::propagation::{Injector, Extractor, TextMapPropagator};
use opentelemetry::global;
use tonic::metadata::MetadataMap;
use tracing_opentelemetry::OpenTelemetrySpanExt;

struct MetadataInjector<'a>(&'a mut MetadataMap);

impl Injector for MetadataInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let Ok(key) = tonic::metadata::MetadataKey::from_bytes(key.as_bytes()) {
            if let Ok(val) = value.parse() {
                self.0.insert(key, val);
            }
        }
    }
}

struct MetadataExtractor<'a>(&'a MetadataMap);

impl Extractor for MetadataExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().filter_map(|k| match k {
            tonic::metadata::KeyRef::Ascii(key) => Some(key.as_str()),
            _ => None,
        }).collect()
    }
}

/// Inject the current span's trace context into tonic metadata.
///
/// Call this in a tonic client interceptor before making a request.
pub fn inject_trace_context(metadata: &mut MetadataMap) {
    let context = tracing::Span::current().context();
    let propagator = opentelemetry_sdk::propagation::TraceContextPropagator::new();
    propagator.inject_context(&context, &mut MetadataInjector(metadata));
}

/// Extract trace context from tonic metadata and set it on the current span.
///
/// Call this in a tonic server layer when receiving a request.
pub fn extract_trace_context(metadata: &MetadataMap) {
    let propagator = opentelemetry_sdk::propagation::TraceContextPropagator::new();
    let context = propagator.extract(&MetadataExtractor(metadata));
    tracing::Span::current().set_parent(context);
}
```

Note: Verify the exact `TraceContextPropagator` import path against the OTEL version used. The API has changed across versions.

**Step 4: Re-export from lib.rs**

```rust
pub use grpc_telemetry::{inject_trace_context, extract_trace_context};
```

**Step 5: Run test to verify it passes**

Run: `cd backend && cargo test -p sober-core -- trace_context`
Expected: PASS

**Step 6: Commit**

```bash
git add backend/crates/sober-core/src/grpc_telemetry.rs backend/crates/sober-core/src/lib.rs
git commit -m "feat(core): add gRPC trace context propagation helpers"
```

---

### Task 6: Create metrics.toml Files for All Crates

**Files:**
- Create: `backend/crates/sober-core/metrics.toml`
- Create: `backend/crates/sober-api/metrics.toml`
- Create: `backend/crates/sober-auth/metrics.toml`
- Create: `backend/crates/sober-agent/metrics.toml`
- Create: `backend/crates/sober-llm/metrics.toml`
- Create: `backend/crates/sober-memory/metrics.toml`
- Create: `backend/crates/sober-scheduler/metrics.toml`
- Create: `backend/crates/sober-crypto/metrics.toml`
- Create: `backend/crates/sober-mind/metrics.toml`
- Create: `backend/crates/sober-plugin/metrics.toml`
- Create: `backend/crates/sober-mcp/metrics.toml`
- Create: `backend/crates/sober-sandbox/metrics.toml`

**Step 1: Create each metrics.toml**

Copy the metric definitions from the design doc (`017-observability/design.md`) into TOML format. Each file follows this structure:

```toml
[crate]
name = "sober-<name>"
dashboard_title = "<Human Readable Title>"

[[metrics]]
name = "sober_<crate>_<metric_name>"
type = "counter|histogram|gauge"
help = "Description"
labels = ["label1", "label2"]
group = "Group Name"
# Optional: buckets = [0.01, 0.05, ...] for histograms
```

Use the full metric inventory from the design doc. For histogram buckets, use these defaults unless specified:

- Duration metrics (`_seconds`): `[0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]`
- Byte metrics (`_bytes`): `[256, 1024, 4096, 16384, 65536, 262144, 1048576, 4194304]`
- Count metrics (iterations, results): `[1, 2, 5, 10, 25, 50, 100]`

Also create a process-level `metrics.toml` covering the `sober_process_*` and `sober_pg_pool_*`/`sober_redis_*`/`sober_qdrant_*` infrastructure metrics. Place this in `sober-core/metrics.toml` since those are shared across all services.

**Step 2: Verify TOML is valid**

Run: `cd backend && for f in crates/*/metrics.toml; do echo "--- $f ---" && python3 -c "import tomllib; tomllib.load(open('$f', 'rb'))"; done`
Expected: No errors.

**Step 3: Commit**

```bash
git add backend/crates/*/metrics.toml
git commit -m "docs(metrics): add metrics.toml definitions for all crates"
```

---

### Task 7: Create the Dashboard Generator Tool

**Files:**
- Create: `tools/dashboard-gen/Cargo.toml`
- Create: `tools/dashboard-gen/src/main.rs`

This is a standalone Rust binary (not part of the backend workspace) that reads `metrics.toml` files and outputs Grafana dashboard JSON.

**Step 1: Create Cargo.toml**

```toml
[package]
name = "dashboard-gen"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
clap = { version = "4", features = ["derive"] }
glob = "0.3"
```

Check crates.io for latest versions before using these.

**Step 2: Write the failing test**

In `src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_metrics_toml() {
        let input = r#"
[crate]
name = "sober-llm"
dashboard_title = "LLM Engine"

[[metrics]]
name = "sober_llm_request_total"
type = "counter"
help = "Total LLM API requests"
labels = ["provider", "model", "status"]
group = "Requests"
"#;
        let registry: MetricsRegistry = toml::from_str(input).unwrap();
        assert_eq!(registry.crate_info.name, "sober-llm");
        assert_eq!(registry.metrics.len(), 1);
        assert_eq!(registry.metrics[0].metric_type, MetricType::Counter);
    }

    #[test]
    fn generates_valid_grafana_json() {
        let registry = MetricsRegistry {
            crate_info: CrateInfo {
                name: "sober-llm".to_owned(),
                dashboard_title: "LLM Engine".to_owned(),
            },
            metrics: vec![
                MetricDef {
                    name: "sober_llm_request_total".to_owned(),
                    metric_type: MetricType::Counter,
                    help: "Total requests".to_owned(),
                    labels: vec!["provider".to_owned()],
                    group: "Requests".to_owned(),
                    buckets: None,
                },
            ],
        };

        let dashboard = generate_dashboard(&registry);
        let json: serde_json::Value = serde_json::from_str(&dashboard).unwrap();

        assert_eq!(json["title"], "LLM Engine");
        assert!(json["panels"].is_array());
        assert!(!json["panels"].as_array().unwrap().is_empty());
    }
}
```

**Step 3: Run test to verify it fails**

Run: `cd tools/dashboard-gen && cargo test`
Expected: FAIL — types not defined.

**Step 4: Implement the data model**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct MetricsRegistry {
    #[serde(rename = "crate")]
    pub crate_info: CrateInfo,
    #[serde(rename = "metrics", default)]
    pub metrics: Vec<MetricDef>,
}

#[derive(Debug, Deserialize)]
pub struct CrateInfo {
    pub name: String,
    pub dashboard_title: String,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MetricType {
    Counter,
    Histogram,
    Gauge,
}

#[derive(Debug, Deserialize)]
pub struct MetricDef {
    pub name: String,
    #[serde(rename = "type")]
    pub metric_type: MetricType,
    pub help: String,
    #[serde(default)]
    pub labels: Vec<String>,
    pub group: String,
    #[serde(default)]
    pub buckets: Option<Vec<f64>>,
}
```

**Step 5: Implement the dashboard generator**

```rust
pub fn generate_dashboard(registry: &MetricsRegistry) -> String {
    // Group metrics by their `group` field
    // For each group, create a Grafana "row" panel followed by metric panels
    // Panel type depends on metric type:
    //   counter -> rate() time series panel
    //   histogram -> heatmap + p50/p95/p99 time series
    //   gauge -> stat panel + time series
    // Generate variable dropdowns from label definitions
    // Output valid Grafana dashboard JSON

    // ... implementation details ...
    // Use serde_json::json! macro to build the dashboard structure
    // Each panel needs: id, type, title, gridPos, targets (PromQL), fieldConfig

    todo!() // Replace with full implementation
}
```

The full implementation of `generate_dashboard` should:

1. Build a Grafana dashboard JSON object with `title`, `uid` (derived from crate name), `panels`, `templating`
2. Group metrics by `group` field
3. For each group, emit a row panel (type: `row`)
4. For each metric in the group, emit panels based on type:
   - **Counter**: Time series panel with `rate($metric_name[$__rate_interval])` query
   - **Histogram**: Two panels — a heatmap and a line chart with `histogram_quantile(0.5, ...)`, `histogram_quantile(0.95, ...)`, `histogram_quantile(0.99, ...)`
   - **Gauge**: A stat panel (current value) and a time series panel (history)
5. For metrics ending in `_bytes`, set unit to `bytes` in fieldConfig
6. For metrics ending in `_seconds`, set unit to `s` in fieldConfig
7. Generate template variables for each unique label across all metrics (Prometheus label_values query)
8. Apply label filters in PromQL using template variables: `{label=~"$label"}`
9. Auto-increment panel IDs and gridPos (24-column grid, panels at width 12 or 8)

**Step 6: Implement CLI with clap**

```rust
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "dashboard-gen", about = "Generate Grafana dashboards from metrics.toml files")]
struct Cli {
    /// Root directory to scan for metrics.toml files
    #[arg(short, long, default_value = "backend/crates")]
    input: PathBuf,

    /// Output directory for generated dashboard JSON
    #[arg(short, long, default_value = "infra/grafana/dashboards/generated")]
    output: PathBuf,
}

fn main() {
    let cli = Cli::parse();

    // Glob for all metrics.toml files under input dir
    let pattern = format!("{}/**/metrics.toml", cli.input.display());
    let files: Vec<_> = glob::glob(&pattern)
        .expect("invalid glob pattern")
        .filter_map(Result::ok)
        .collect();

    std::fs::create_dir_all(&cli.output).expect("failed to create output directory");

    for file in &files {
        let content = std::fs::read_to_string(file).expect("failed to read metrics.toml");
        let registry: MetricsRegistry = toml::from_str(&content).expect("invalid metrics.toml");
        let dashboard = generate_dashboard(&registry);

        let output_file = cli.output.join(format!("{}.json", registry.crate_info.name));
        std::fs::write(&output_file, &dashboard).expect("failed to write dashboard JSON");
        println!("Generated: {}", output_file.display());
    }

    println!("Generated {} dashboards from {} metrics.toml files", files.len(), files.len());
}
```

**Step 7: Run tests to verify they pass**

Run: `cd tools/dashboard-gen && cargo test`
Expected: PASS

**Step 8: Verify the tool runs end-to-end**

Run: `cd tools/dashboard-gen && cargo run -- --input ../../backend/crates --output /tmp/dashboards`
Expected: Generates JSON files in `/tmp/dashboards/`, one per crate. Verify at least one with `python3 -c "import json; json.load(open('/tmp/dashboards/sober-llm.json'))"`.

**Step 9: Commit**

```bash
git add tools/dashboard-gen/
git commit -m "feat(tools): add dashboard-gen for auto-generating Grafana dashboards"
```

---

### Task 8: Docker Compose Observability Stack

**Files:**
- Modify: `docker-compose.yml` (add services)

**Step 1: Add observability services**

Add the following services to `docker-compose.yml`:

```yaml
  prometheus:
    image: prom/prometheus:latest
    container_name: sober-prometheus
    volumes:
      - ./infra/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml:ro
      - ./infra/prometheus/alerts/:/etc/prometheus/alerts/:ro
      - prometheus-data:/prometheus
    ports:
      - "9090:9090"
    restart: unless-stopped

  tempo:
    image: grafana/tempo:latest
    container_name: sober-tempo
    volumes:
      - ./infra/tempo/tempo.yml:/etc/tempo/config.yaml:ro
      - tempo-data:/var/tempo
    ports:
      - "4317:4317"
      - "3200:3200"
    command: ["-config.file=/etc/tempo/config.yaml"]
    restart: unless-stopped

  loki:
    image: grafana/loki:latest
    container_name: sober-loki
    volumes:
      - ./infra/loki/loki.yml:/etc/loki/config.yaml:ro
      - loki-data:/loki
    ports:
      - "3100:3100"
    command: ["-config.file=/etc/loki/config.yaml"]
    restart: unless-stopped

  promtail:
    image: grafana/promtail:latest
    container_name: sober-promtail
    volumes:
      - ./infra/promtail/promtail.yml:/etc/promtail/config.yaml:ro
      - /var/run/docker.sock:/var/run/docker.sock:ro
    command: ["-config.file=/etc/promtail/config.yaml"]
    depends_on:
      - loki
    restart: unless-stopped

  grafana:
    image: grafana/grafana:latest
    container_name: sober-grafana
    volumes:
      - ./infra/grafana/provisioning/:/etc/grafana/provisioning/:ro
      - ./infra/grafana/dashboards/:/var/lib/grafana/dashboards/:ro
      - grafana-data:/var/lib/grafana
    ports:
      - "3000:3000"
    environment:
      - GF_AUTH_ANONYMOUS_ENABLED=true
      - GF_AUTH_ANONYMOUS_ORG_ROLE=Viewer
      - GF_SECURITY_ADMIN_PASSWORD=admin
    depends_on:
      - prometheus
      - tempo
      - loki
    restart: unless-stopped
```

Add volumes at the bottom of `docker-compose.yml`:

```yaml
volumes:
  prometheus-data:
  tempo-data:
  loki-data:
  grafana-data:
```

**Step 2: Verify docker compose config is valid**

Run: `docker compose config --quiet`
Expected: No errors.

**Step 3: Commit**

```bash
git add docker-compose.yml
git commit -m "infra(docker): add Prometheus, Tempo, Loki, Promtail, Grafana services"
```

---

### Task 9: Create Prometheus Configuration

**Files:**
- Create: `infra/prometheus/prometheus.yml`
- Create: `infra/prometheus/alerts/critical.yml`
- Create: `infra/prometheus/alerts/warning.yml`

**Step 1: Create prometheus.yml**

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

rule_files:
  - /etc/prometheus/alerts/*.yml

scrape_configs:
  - job_name: "sober-api"
    static_configs:
      - targets: ["host.docker.internal:3001"]
        labels:
          service: "sober-api"

  - job_name: "sober-scheduler"
    static_configs:
      - targets: ["host.docker.internal:3002"]
        labels:
          service: "sober-scheduler"

  - job_name: "sober-agent"
    static_configs:
      - targets: ["host.docker.internal:3003"]
        labels:
          service: "sober-agent"
```

Note: Ports are placeholders — adjust to match the actual `/metrics` bind addresses once services are running. During dev, `host.docker.internal` reaches the host machine from inside Docker.

**Step 2: Create critical.yml**

```yaml
groups:
  - name: sober_critical
    rules:
      - alert: ServiceDown
        expr: up == 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "Service {{ $labels.job }} is down"
          description: "{{ $labels.job }} has been unreachable for more than 1 minute."

      - alert: HighErrorRate
        expr: >
          sum(rate(sober_api_request_total{status=~"5.."}[5m])) /
          sum(rate(sober_api_request_total[5m])) > 0.05
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "High 5xx error rate ({{ $value | humanizePercentage }})"

      - alert: LLMProviderDown
        expr: >
          sum(rate(sober_llm_request_total{status="error"}[5m])) by (provider) /
          sum(rate(sober_llm_request_total[5m])) by (provider) > 0.9
        for: 3m
        labels:
          severity: critical
        annotations:
          summary: "LLM provider {{ $labels.provider }} error rate > 90%"

      - alert: DatabaseConnectionExhausted
        expr: sober_pg_pool_connections_idle == 0
        for: 2m
        labels:
          severity: critical
        annotations:
          summary: "PostgreSQL connection pool has zero idle connections"

      - alert: InjectionDetected
        expr: rate(sober_mind_injection_detections_total[1m]) > 0
        labels:
          severity: critical
        annotations:
          summary: "Prompt injection attempt detected"

      - alert: SandboxViolation
        expr: rate(sober_sandbox_policy_violations_total[1m]) > 0
        labels:
          severity: critical
        annotations:
          summary: "Sandbox policy violation: {{ $labels.violation }} in profile {{ $labels.profile }}"
```

**Step 3: Create warning.yml**

```yaml
groups:
  - name: sober_warning
    rules:
      - alert: HighP95Latency
        expr: histogram_quantile(0.95, rate(sober_api_request_duration_seconds_bucket[5m])) > 5
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "API p95 latency is {{ $value | humanizeDuration }}"

      - alert: LLMLatencyDegraded
        expr: histogram_quantile(0.95, rate(sober_llm_request_duration_seconds_bucket[5m])) > 15
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "LLM p95 latency is {{ $value | humanizeDuration }}"

      - alert: SchedulerJobLag
        expr: histogram_quantile(0.95, rate(sober_scheduler_job_lag_seconds_bucket[5m])) > 60
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Scheduler job lag p95 is {{ $value | humanizeDuration }}"

      - alert: HighMemoryUsage
        expr: sober_process_resident_memory_bytes > 1e9
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "Process {{ $labels.job }} using {{ $value | humanize1024 }} memory"

      - alert: ConnectionPoolPressure
        expr: >
          sober_pg_pool_connections_idle /
          (sober_pg_pool_connections_active + sober_pg_pool_connections_idle) < 0.1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "PostgreSQL connection pool under pressure ({{ $value | humanizePercentage }} idle)"

      - alert: AuthFailureSpike
        expr: >
          rate(sober_auth_attempts_total{status="failure"}[5m]) >
          3 * avg_over_time(rate(sober_auth_attempts_total{status="failure"}[5m])[1h:5m])
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Auth failure rate spiked 3x above baseline"

      - alert: PruningBacklog
        expr: >
          increase(sober_memory_chunks_total[30m]) > 0
          and increase(sober_memory_pruned_chunks_total[30m]) == 0
        for: 30m
        labels:
          severity: warning
        annotations:
          summary: "Memory chunks growing but pruning is removing nothing"

      - alert: MissedSchedulerJobs
        expr: rate(sober_scheduler_missed_executions_total[5m]) > 0
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Scheduler is missing job executions"
```

**Step 4: Commit**

```bash
git add infra/prometheus/
git commit -m "infra(prometheus): add scrape config and alerting rules"
```

---

### Task 10: Create Tempo, Loki, and Promtail Configs

**Files:**
- Create: `infra/tempo/tempo.yml`
- Create: `infra/loki/loki.yml`
- Create: `infra/promtail/promtail.yml`

**Step 1: Create tempo.yml**

```yaml
server:
  http_listen_port: 3200

distributor:
  receivers:
    otlp:
      protocols:
        grpc:
          endpoint: "0.0.0.0:4317"

storage:
  trace:
    backend: local
    local:
      path: /var/tempo/traces
    wal:
      path: /var/tempo/wal

metrics_generator:
  storage:
    path: /var/tempo/metrics
```

Check Grafana Tempo docs for the latest config format — the schema evolves between versions.

**Step 2: Create loki.yml**

```yaml
auth_enabled: false

server:
  http_listen_port: 3100

common:
  path_prefix: /loki
  storage:
    filesystem:
      chunks_directory: /loki/chunks
      rules_directory: /loki/rules
  replication_factor: 1
  ring:
    kvstore:
      store: inmemory

schema_config:
  configs:
    - from: "2024-01-01"
      store: tsdb
      object_store: filesystem
      schema: v13
      index:
        prefix: index_
        period: 24h
```

Check Grafana Loki docs for the latest config format.

**Step 3: Create promtail.yml**

```yaml
server:
  http_listen_port: 9080

positions:
  filename: /tmp/positions.yaml

clients:
  - url: http://loki:3100/loki/api/v1/push

scrape_configs:
  - job_name: docker
    docker_sd_configs:
      - host: unix:///var/run/docker.sock
        refresh_interval: 5s
    relabel_configs:
      - source_labels: ["__meta_docker_container_name"]
        target_label: "container"
      - source_labels: ["__meta_docker_container_label_com_docker_compose_service"]
        target_label: "service"
    pipeline_stages:
      - json:
          expressions:
            level: level
            message: message
            timestamp: timestamp
            target: target
      - labels:
          level:
          target:
```

**Step 4: Commit**

```bash
git add infra/tempo/ infra/loki/ infra/promtail/
git commit -m "infra(telemetry): add Tempo, Loki, and Promtail configurations"
```

---

### Task 11: Create Grafana Provisioning

**Files:**
- Create: `infra/grafana/provisioning/datasources/datasources.yml`
- Create: `infra/grafana/provisioning/dashboards/dashboards.yml`
- Create: `infra/grafana/dashboards/generated/.gitkeep`
- Create: `infra/grafana/dashboards/curated/.gitkeep`

**Step 1: Create datasources.yml**

```yaml
apiVersion: 1

datasources:
  - name: Prometheus
    type: prometheus
    access: proxy
    url: http://prometheus:9090
    isDefault: true
    editable: false

  - name: Tempo
    type: tempo
    access: proxy
    url: http://tempo:3200
    editable: false
    jsonData:
      tracesToLogsV2:
        datasourceUid: loki
        filterByTraceID: true
      tracesToMetrics:
        datasourceUid: prometheus
      serviceMap:
        datasourceUid: prometheus

  - name: Loki
    type: loki
    access: proxy
    url: http://loki:3100
    editable: false
    jsonData:
      derivedFields:
        - datasourceUid: tempo
          matcherRegex: '"trace_id":"(\w+)"'
          name: TraceID
          url: "$${__value.raw}"
```

This wires up cross-linking: click a trace in Tempo to see related Loki logs, click a log line with a trace_id to jump to the trace.

**Step 2: Create dashboards.yml**

```yaml
apiVersion: 1

providers:
  - name: "Generated"
    orgId: 1
    folder: "Generated"
    type: file
    disableDeletion: false
    updateIntervalSeconds: 30
    options:
      path: /var/lib/grafana/dashboards/generated
      foldersFromFilesStructure: false

  - name: "Curated"
    orgId: 1
    folder: "Curated"
    type: file
    disableDeletion: false
    updateIntervalSeconds: 30
    options:
      path: /var/lib/grafana/dashboards/curated
      foldersFromFilesStructure: false
```

**Step 3: Create .gitkeep files**

```bash
mkdir -p infra/grafana/dashboards/generated infra/grafana/dashboards/curated
touch infra/grafana/dashboards/generated/.gitkeep
touch infra/grafana/dashboards/curated/.gitkeep
```

**Step 4: Commit**

```bash
git add infra/grafana/
git commit -m "infra(grafana): add datasource provisioning and dashboard directories"
```

---

### Task 12: Create Curated Overview Dashboard

**Files:**
- Create: `infra/grafana/dashboards/curated/overview.json`

**Step 1: Build the overview dashboard JSON**

This is the hand-crafted "system at a glance" dashboard. Build it as a Grafana dashboard JSON with:

- **uid:** `sober-overview`
- **title:** `Sober - System Overview`
- **5 rows** with panels as described in the design doc:

**Row 1 — System Health (4 panels, width 6 each):**
1. Stat: Service uptime — `sober_process_uptime_seconds` per job
2. Time series: Error rate — `rate(sober_api_request_total{status=~"5.."}[5m])`
3. Stat: Active WS connections — `sober_api_ws_connections_active`
4. Time series: Request rate — `rate(sober_api_request_total[5m])`

**Row 2 — Agent Performance (4 panels, width 6 each):**
1. Time series: Agent request rate + p95 latency (dual axis)
2. Time series: LLM p95 latency + tokens/min (dual axis)
3. Gauge: Tool call success rate — `rate(sober_agent_tool_calls_total{status="success"}[5m]) / rate(sober_agent_tool_calls_total[5m])`
4. Stat: LLM cost 24h — `increase(sober_llm_estimated_cost_dollars_total[24h])`

**Row 3 — Memory & Storage (4 panels, width 6 each):**
1. Time series: Vector search p95 — `histogram_quantile(0.95, rate(sober_memory_search_duration_seconds_bucket[5m]))`
2. Bar gauge: Chunks by type — `sober_memory_chunks_total` by `chunk_type`
3. Time series: Pruning activity — `rate(sober_memory_pruned_chunks_total[5m])`
4. Gauge: Connection pool utilization — idle / (active + idle)

**Row 4 — Scheduler (4 panels, width 6 each):**
1. Stat: Registered jobs — `sober_scheduler_jobs_registered`
2. Time series: Job lag p95
3. Stat: Missed executions — `increase(sober_scheduler_missed_executions_total[1h])`
4. Time series: Tick duration — `histogram_quantile(0.95, rate(sober_scheduler_tick_duration_seconds_bucket[5m]))`

**Row 5 — Security (4 panels, width 6 each):**
1. Time series: Auth attempts (stacked) — success vs failure
2. Stat: Injection detections — `increase(sober_mind_injection_detections_total[24h])`
3. Stat: Sandbox violations — `increase(sober_sandbox_policy_violations_total[24h])`
4. Time series: Permission denials — `rate(sober_auth_permission_checks_total{result="denied"}[5m])`

Use `serde_json::json!` or write the JSON directly. Each panel needs `id`, `type`, `title`, `gridPos` (x, y, w, h), `targets` with PromQL `expr`, and `fieldConfig`.

**Step 2: Validate JSON**

Run: `python3 -c "import json; json.load(open('infra/grafana/dashboards/curated/overview.json'))"`
Expected: No errors.

**Step 3: Commit**

```bash
git add infra/grafana/dashboards/curated/overview.json
git commit -m "infra(grafana): add curated system overview dashboard"
```

---

### Task 13: Add Justfile Commands

**Files:**
- Modify: `justfile`

**Step 1: Add dashboard generation command**

```just
# Generate Grafana dashboards from metrics.toml files
dashboards:
    cd tools/dashboard-gen && cargo run -- \
        --input ../../backend/crates \
        --output ../../infra/grafana/dashboards/generated

# Start observability stack (Prometheus, Grafana, Tempo, Loki)
observability-up:
    docker compose up -d prometheus tempo loki promtail grafana

# Stop observability stack
observability-down:
    docker compose stop prometheus tempo loki promtail grafana
```

**Step 2: Verify commands are recognized**

Run: `just --list | grep -E "dashboards|observability"`
Expected: All three commands listed.

**Step 3: Commit**

```bash
git add justfile
git commit -m "chore(just): add dashboards and observability-up/down commands"
```

---

### Task 14: Generate Initial Dashboards and Verify Stack

**Step 1: Generate dashboards**

Run: `just dashboards`
Expected: One JSON file per crate in `infra/grafana/dashboards/generated/`.

**Step 2: Start the observability stack**

Run: `just observability-up`
Expected: All containers start. Verify with `docker compose ps`.

**Step 3: Verify Grafana loads dashboards**

Open `http://localhost:3000` in a browser. Check:
- [ ] Datasources page shows Prometheus, Tempo, Loki (all green)
- [ ] Dashboard list shows "Generated" folder with per-crate dashboards
- [ ] Dashboard list shows "Curated" folder with the overview dashboard
- [ ] Overview dashboard loads without query errors (panels will show "No data" since no app is running, but no red error badges)

**Step 4: Verify Prometheus**

Open `http://localhost:9090/targets`. Targets will show as DOWN (no app running) — that's expected. The config should be loaded without errors.

**Step 5: Commit generated dashboards**

```bash
git add infra/grafana/dashboards/generated/
git commit -m "chore(dashboards): generate initial Grafana dashboards from metrics.toml"
```

---

### Task 15: Final Verification and Cleanup

**Step 1: Run clippy on dashboard-gen**

Run: `cd tools/dashboard-gen && cargo clippy -- -D warnings`
Expected: No warnings.

**Step 2: Run tests**

Run: `cd tools/dashboard-gen && cargo test`
Expected: All tests pass.

**Step 3: Run backend clippy**

Run: `cd backend && cargo clippy -p sober-core -- -D warnings`
Expected: No warnings (telemetry code included).

**Step 4: Run backend tests**

Run: `cd backend && cargo test -p sober-core`
Expected: All tests pass (including new telemetry tests).

**Step 5: Verify graceful degradation**

Stop the observability stack: `just observability-down`

If a Sober service were running, it should:
- Still write logs to stdout
- Still serve `/metrics` (Prometheus just isn't scraping)
- Not crash or block if Tempo is unreachable (OTEL exporter should fail gracefully)

This will be fully testable once the services are implemented. For now, verify that `init_telemetry()` with an unreachable OTEL endpoint does not panic:

```rust
#[test]
fn init_telemetry_with_unreachable_otel_does_not_panic() {
    let config = TelemetryConfig {
        otel_endpoint: Some("http://localhost:99999".to_owned()),
        service_name: "test".to_owned(),
        traces_sampler: "always_on".to_owned(),
        metrics_listen_addr: None,
    };
    let _guard = init_telemetry(&config);
    // Should not panic — OTEL uses batch exporter that fails silently
}
```

**Step 6: Final commit if any fixes were needed**

```bash
git add -A
git commit -m "fix(telemetry): address clippy and test issues"
```

---

## Acceptance Criteria

- [ ] `init_telemetry()` in `sober-core` configures tracing, OTEL export, and Prometheus recorder
- [ ] `TelemetryConfig` reads from env vars with sensible defaults
- [ ] gRPC trace propagation helpers (`inject_trace_context`, `extract_trace_context`) compile and pass roundtrip test
- [ ] `/metrics` handler serves Prometheus format
- [ ] Each crate has a `metrics.toml` defining its metrics
- [ ] `tools/dashboard-gen` reads `metrics.toml` and outputs valid Grafana dashboard JSON
- [ ] Docker Compose includes Prometheus, Tempo, Loki, Promtail, Grafana
- [ ] Grafana auto-loads generated + curated dashboards on startup
- [ ] Alerting rules (critical + warning) provisioned in Prometheus
- [ ] Curated overview dashboard has 5 rows with all specified panels
- [ ] `just dashboards` regenerates all dashboard JSON
- [ ] `just observability-up` / `just observability-down` manage the stack
- [ ] System does not crash when OTEL/Prometheus backends are unreachable
- [ ] All clippy warnings resolved, all tests pass
