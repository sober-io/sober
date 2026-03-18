# Plan #018: Observability — Metrics + Distributed Tracing

## Context

The observability design (`docs/plans/pending/018-observability/design.md`) defines ~100 metrics across all crates, Docker infrastructure (Prometheus, Tempo, Grafana), gRPC trace propagation, and auto-generated dashboards. Plan 003 implemented `init_telemetry()` in `sober-core/src/telemetry.rs` — but no binary uses it. Each of the 4 binaries has its own local `init_tracing()` with basic logging only: no Prometheus, no OTEL, no `/metrics` endpoint, and zero `metrics::counter!()` / `metrics::histogram!()` calls exist in the codebase.

This plan does three things: (1) plumbing — migrate binaries to centralized telemetry, expose `/metrics`, add gRPC trace propagation; (2) instrumentation — add `metrics::*` recording calls at every code path defined in the design doc; (3) infrastructure — Docker services, dashboards, alerting. **No Loki/Promtail** — logs stay on stdout.

---

## Parallel Streams

Five independent streams — no shared file modifications between them:

| Stream | Scope | Files touched |
|--------|-------|---------------|
| **A** | Core telemetry + binary migration + gRPC propagation | `sober-core/`, all 4 binary `main.rs`, `sober-api/src/state.rs` |
| **B** | Docker infra configs | `docker-compose.yml`, `infra/prometheus/`, `infra/tempo/`, `infra/grafana/` |
| **C** | Per-crate metric instrumentation + `metrics.toml` | `sober-api/`, `sober-agent/`, `sober-llm/`, `sober-auth/`, `sober-scheduler/`, `sober-memory/`, `sober-mind/`, `sober-crypto/`, `sober-mcp/`, `sober-sandbox/`, `sober-db/` (library source files + Cargo.toml + metrics.toml) |
| **D** | Dashboard generator tool | `tools/dashboard-gen/` |
| **E** | Justfile + plan lifecycle | `justfile`, `docs/plans/` |

**Dependency:** Stream A must complete before C (binaries need init_telemetry before metrics are useful). Streams B, D, E are fully independent.

---

## Stream A: Core Telemetry + Binary Migration

### A1. Simplify `init_telemetry` signature

**File:** `backend/crates/sober-core/src/telemetry.rs`

Current: `pub fn init_telemetry(config: &AppConfig) -> TelemetryGuard` — only uses `config.environment`.

Change to:
```rust
pub fn init_telemetry(environment: Environment, default_filter: &str) -> TelemetryGuard
```

- Line 69: use `default_filter` param instead of hardcoded `"info"`
- Line 87: match on `environment` param instead of `config.environment`
- Remove `use crate::config::AppConfig` (keep `Environment`)

### A2. Add `spawn_metrics_server` helper

**File:** `backend/crates/sober-core/src/telemetry.rs`

For gRPC-only binaries (agent, scheduler) that need an HTTP endpoint for Prometheus scraping:

```rust
pub fn spawn_metrics_server(handle: PrometheusHandle, port: u16) {
    tokio::spawn(async move {
        let app = Router::new()
            .route("/metrics", axum::routing::get(MetricsEndpoint(handle)));
        let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await
            .expect("metrics server bind failed");
        tracing::info!(port, "metrics server listening");
        axum::serve(listener, app).await.ok();
    });
}
```

Add `tokio` dep to sober-core Cargo.toml (for spawn + TcpListener).
Export from `lib.rs`: `pub use telemetry::spawn_metrics_server;`

### A3. Add gRPC trace propagation

**File:** `backend/crates/sober-core/src/telemetry.rs` (append to existing module)

**Dependency:** Add `tonic = { workspace = true }` to `sober-core/Cargo.toml`

Add:
- `MetadataMapInjector` — implements `opentelemetry::propagation::Injector` for tonic `MetadataMap`
- `MetadataMapExtractor` — implements `opentelemetry::propagation::Extractor`
- `inject_trace_context(metadata: &mut MetadataMap)` — injects current span context
- `extract_trace_context(metadata: &MetadataMap)` — extracts and sets parent context

Also register the global propagator in `init_telemetry()`:
```rust
opentelemetry::global::set_text_map_propagator(
    opentelemetry_sdk::propagation::TraceContextPropagator::new()
);
```

### A4. Migrate sober-api

**File:** `backend/crates/sober-api/src/main.rs`

- Delete `init_tracing()` (lines 88-106)
- Replace line 27: `let telemetry = sober_core::init_telemetry(config.environment, "sober_api=debug,tower_http=debug,info");`
- Mount `/metrics` at top level (outside `/api/v1` nest):
  ```rust
  let app = routes::build_router(state.clone())
      .route("/metrics", get(sober_core::MetricsEndpoint(telemetry.prometheus.clone())));
  ```

**File:** `backend/crates/sober-api/src/state.rs`
- Add trace context injection to agent gRPC client in `connect_agent()`

### A5. Migrate sober-agent

**File:** `backend/crates/sober-agent/src/main.rs`

- Delete `init_tracing()` (lines 308-324)
- Replace line 43: `let telemetry = sober_core::init_telemetry(config.environment, "sober_agent=info,sober_mind=info,sober_memory=info");`
- Spawn metrics server: `sober_core::spawn_metrics_server(telemetry.prometheus.clone(), env_or_default("METRICS_PORT", 9100));`
- Apply trace propagation on gRPC server + scheduler client

### A6. Migrate sober-scheduler

**File:** `backend/crates/sober-scheduler/src/main.rs`

- Delete `init_tracing()` (lines 253-268)
- Replace line 41: `let telemetry = sober_core::init_telemetry(config.environment, "sober_scheduler=info");`
- Spawn metrics server on port 9101
- Apply trace propagation on gRPC server + agent client

### A7. Migrate sober-web

**File:** `backend/crates/sober-web/src/main.rs`

- Delete `init_tracing()` (lines 260-266)
- Read environment from `SOBER_ENV` env var directly (sober-web doesn't use `AppConfig`)
- `let _telemetry = sober_core::init_telemetry(environment, "sober_web=info");`

### A8. Verify

```bash
cd backend && cargo build -q && cargo clippy -q -- -D warnings && cargo test -p sober-core -q
```

---

## Stream B: Docker Infrastructure

### B1. Prometheus

**Create:** `infra/prometheus/prometheus.yml` — scrape `sober-api:3000`, `sober-agent:9100`, `sober-scheduler:9101`

**Create:** `infra/prometheus/alerts/critical.yml`
- ServiceDown, HighErrorRate, LLMProviderDown, DatabaseConnectionExhausted, InjectionDetected, SandboxViolation

**Create:** `infra/prometheus/alerts/warning.yml`
- HighP95Latency, LLMLatencyDegraded, SchedulerJobLag, HighMemoryUsage, ConnectionPoolPressure, AuthFailureSpike

### B2. Tempo

**Create:** `infra/tempo/tempo.yml` — OTLP gRPC on 4317, query on 3200, local storage

### B3. Grafana

**Create:** `infra/grafana/provisioning/datasources/datasources.yml` — Prometheus + Tempo (with trace-to-metrics linking)
**Create:** `infra/grafana/provisioning/dashboards/dashboards.yml` — auto-load from `/var/lib/grafana/dashboards/`
**Create:** `infra/grafana/dashboards/generated/.gitkeep`
**Create:** `infra/grafana/dashboards/curated/overview.json` — 5-row overview dashboard per design doc

### B4. Docker Compose

**Modify:** `docker-compose.yml`
- Add `prometheus`, `tempo`, `grafana` services
- Add `prometheus_data`, `tempo_data`, `grafana_data` volumes
- Add OTEL env vars to application services:
  - `x-common-env`: `OTEL_EXPORTER_OTLP_ENDPOINT: http://tempo:4317`
  - Per-service: `OTEL_SERVICE_NAME`, `METRICS_PORT` (agent: 9100, scheduler: 9101)
  - Expose metrics ports for agent + scheduler

---

## Stream C: Per-Crate Metric Instrumentation

For each crate: (1) add `metrics = { workspace = true }` to Cargo.toml, (2) add `metrics::counter!()` / `metrics::histogram!()` calls at code paths, (3) create `metrics.toml` declaration file.

Metric names follow design doc convention: `sober_<crate>_<noun>_<unit>`.

### C1. sober-api — HTTP/WebSocket Gateway

**Add `metrics` dep to:** `backend/crates/sober-api/Cargo.toml`

**HTTP request middleware** (new middleware or extend existing TraceLayer):
- Create axum middleware layer that records for every request:
  - `sober_api_request_duration_seconds` (histogram) — labels: method, path, status
  - `sober_api_request_total` (counter) — labels: method, path, status
  - `sober_api_requests_in_flight` (gauge) — increment on entry, decrement on exit

**WebSocket** in `routes/ws.rs` (`handle_socket()`, line 226+):
- `sober_api_ws_connections_active` (gauge) — increment on open, decrement on close
- `sober_api_ws_connections_total` (counter) — labels: status (opened/closed/error)
- `sober_api_ws_messages_total` (counter) — labels: direction (inbound/outbound)

**Rate limiting** in `middleware/rate_limit.rs` (`RateLimitService::call()`, line 95):
- `sober_api_rate_limit_hits_total` (counter) — on rejection

**Admin socket** in admin handler:
- `sober_api_admin_commands_total` (counter) — labels: command, status

### C2. sober-agent — Orchestration

**Add `metrics` dep to:** `backend/crates/sober-agent/Cargo.toml`

**Agent loop** in `agent.rs` (`Agent::handle_message()`, line 283+):
- `sober_agent_requests_total` (counter) — labels: trigger, status
- `sober_agent_request_duration_seconds` (histogram)

**Agent loop iterations** in `agent.rs` (`Agent::run_loop_streaming()`, line 495):
- `sober_agent_loop_iterations_total` (counter)
- `sober_agent_loop_iterations_per_request` (histogram)

**Tool execution** in `agent.rs` (lines 1015-1057):
- `sober_agent_tool_calls_total` (counter) — labels: tool, status (success/error/timeout/not_found)
- `sober_agent_tool_call_duration_seconds` (histogram) — labels: tool

**Deferred** (features not yet implemented): replica management, task queue metrics.

### C3. sober-llm — LLM Provider

**Add `metrics` dep to:** `backend/crates/sober-llm/Cargo.toml`

**Completion** in `client.rs` (`complete()`, line 184):
- `sober_llm_request_total` (counter) — labels: provider, model, status
- `sober_llm_request_duration_seconds` (histogram) — labels: provider, model
- `sober_llm_tokens_input_total` (counter) — labels: provider, model
- `sober_llm_tokens_output_total` (counter) — labels: provider, model

**Streaming** in `client.rs` (`stream()`, line 209):
- Same counters, recorded when stream completes

**Embeddings** in `client.rs` (`embed()`, line 236):
- `sober_llm_embed_request_total` (counter) — labels: provider
- `sober_llm_embed_request_duration_seconds` (histogram)

**Deferred**: cost estimation (requires pricing data), time-to-first-token (requires stream refactor), retry metrics.

### C4. sober-auth — Authentication

**Add `metrics` dep to:** `backend/crates/sober-auth/Cargo.toml`

**Login** in `service.rs` (`login()`, line 89):
- `sober_auth_attempts_total` (counter) — labels: method, status (success/failure/locked)
- `sober_auth_attempt_duration_seconds` (histogram) — labels: method

**Sessions** in `service.rs`:
- `sober_auth_sessions_created_total` (counter) — in register/login success path
- `sober_auth_sessions_expired_total` (counter) — in session expiry/cleanup

**Authorization** in permission check middleware:
- `sober_auth_permission_checks_total` (counter) — labels: permission, result

### C5. sober-scheduler — Tick Engine

**Add `metrics` dep to:** `backend/crates/sober-scheduler/Cargo.toml`

**Tick loop** in `engine.rs` (`run()`, line 106 + `tick()`, line 129):
- `sober_scheduler_ticks_total` (counter)
- `sober_scheduler_tick_duration_seconds` (histogram)
- `sober_scheduler_jobs_due_per_tick` (histogram)

**Job execution** in `engine.rs`:
- `sober_scheduler_job_executions_total` (counter) — labels: job_name, status
- `sober_scheduler_job_duration_seconds` (histogram) — labels: job_name
- `sober_scheduler_job_lag_seconds` (histogram)

**State**:
- `sober_scheduler_jobs_registered` (gauge) — labels: type, persistence
- `sober_scheduler_paused` (gauge)

### C6. sober-memory — Vector Storage

**Add `metrics` dep to:** `backend/crates/sober-memory/Cargo.toml`

**Search** in `store/memory_store.rs` (`search()`, line 131):
- `sober_memory_search_total` (counter) — labels: scope, search_type
- `sober_memory_search_duration_seconds` (histogram)
- `sober_memory_search_results_count` (histogram)

**Storage** in `store/memory_store.rs` (`store()`, line 87):
- `sober_memory_chunks_stored_total` (counter) — labels: chunk_type, scope

**Pruning** in `store/memory_store.rs` (`prune()`, line 226):
- `sober_memory_prune_runs_total` (counter)
- `sober_memory_prune_duration_seconds` (histogram)
- `sober_memory_pruned_chunks_total` (counter)

**Deferred**: BCF encode/decode (not yet implemented), chunk gauge, storage bytes.

### C7. sober-mind — Prompt Assembly

**Add `metrics` dep to:** `backend/crates/sober-mind/Cargo.toml`

**Prompt assembly** in `assembly.rs` (`Mind::assemble()`, line 53):
- `sober_mind_prompt_assembly_duration_seconds` (histogram) — labels: trigger
- `sober_mind_prompt_token_estimate` (histogram)

**Injection detection** in `injection.rs` (`classify_input()`, line 86):
- `sober_mind_injection_detections_total` (counter) — on Rejected/Flagged verdicts

### C8. sober-crypto — Cryptographic Operations

**Add `metrics` dep to:** `backend/crates/sober-crypto/Cargo.toml`

**Sign/Verify** in `keys.rs`:
- `sober_crypto_sign_total` (counter)
- `sober_crypto_verify_total` (counter) — labels: result (valid/invalid)

**Encrypt/Decrypt** in `envelope.rs`:
- `sober_crypto_encrypt_total` (counter)
- `sober_crypto_decrypt_total` (counter)
- `sober_crypto_keypair_generated_total` (counter)

Duration histograms deferred — crypto ops are sub-microsecond, histogram overhead may dominate.

### C9. sober-mcp — MCP Tool Interop

**Add `metrics` dep to:** `backend/crates/sober-mcp/Cargo.toml`

**Tool calls** in `client.rs` (`call_tool()`, line 188):
- `sober_mcp_tool_calls_total` (counter) — labels: server, tool, status
- `sober_mcp_tool_call_duration_seconds` (histogram) — labels: server, tool

**Connections** in `client.rs` (`connect()`, line 58):
- `sober_mcp_server_connections_active` (gauge) — labels: server
- `sober_mcp_server_reconnects_total` (counter) — labels: server

### C10. sober-sandbox — Process Sandboxing

**Add `metrics` dep to:** `backend/crates/sober-sandbox/Cargo.toml`

**Execution** in `bwrap.rs` (`execute()`, line 55):
- `sober_sandbox_executions_total` (counter) — labels: profile, status
- `sober_sandbox_execution_duration_seconds` (histogram) — labels: profile

**Policy violations**:
- `sober_sandbox_policy_violations_total` (counter) — labels: profile, violation

### C11. sober-db — Connection Pool

**Add `metrics` dep to:** `backend/crates/sober-db/Cargo.toml`

**Pool** in `pool.rs` (`create_pool()`, line 20):
- `sober_pg_pool_connections_active` (gauge) — via periodic sqlx pool stats
- `sober_pg_pool_connections_idle` (gauge)

**Deferred**: per-query instrumentation (too invasive for this plan), Redis (not in use), Qdrant pool metrics.

### C12. metrics.toml files

Create a `metrics.toml` in each crate root documenting the metrics it emits (input for dashboard generator). Follow the design doc format:

```toml
[crate]
name = "sober-llm"
dashboard_title = "LLM Engine"

[[metrics]]
name = "sober_llm_request_total"
type = "counter"
help = "Total LLM API requests"
labels = ["provider", "model", "status"]
group = "Requests"
```

**13 files** (all crates except sober-plugin and sober-cli).

---

## Stream D: Dashboard Generator Tool

**Create:** `tools/dashboard-gen/Cargo.toml` (standalone, not in backend workspace)
**Create:** `tools/dashboard-gen/src/main.rs`

- Parse `metrics.toml` files (serde + toml crate)
- Generate Grafana dashboard JSON per crate
- Panel type rules:
  - `counter` (_total) → rate() timeseries
  - `histogram` (_seconds, _bytes) → heatmap + p50/p95/p99 timeseries
  - `gauge` (_active, _registered) → stat + timeseries
- Template variables from label definitions
- CLI: `--input <crates-dir>` `--output <output-dir>`

---

## Stream E: Justfile + Plan Lifecycle

**Modify:** `justfile` — add `dashboards`, `observability-up`, `observability-down` commands

**Move:** `docs/plans/pending/018-observability/` → `docs/plans/active/018-observability/` (first commit)

---

## Verification

1. `cd backend && cargo build -q` — all crates compile
2. `cd backend && cargo clippy -q -- -D warnings` — no warnings
3. `cd backend && cargo test --workspace -q` — all tests pass
4. `cd tools/dashboard-gen && cargo test -q` — dashboard gen tests pass
5. `docker compose up -d` — all services start including observability
6. `curl localhost:3000/metrics` — sober-api returns Prometheus metrics including `sober_api_request_total`
7. `curl localhost:9100/metrics` — sober-agent returns metrics
8. `curl localhost:9101/metrics` — sober-scheduler returns metrics
9. Grafana at `localhost:3001` shows Prometheus + Tempo datasources green
10. Make an API request → verify trace appears in Tempo with spans from api + agent
11. `just dashboards` generates dashboard JSON files
12. `just observability-up` / `just observability-down` work

---

## Critical Files

| File | Action |
|------|--------|
| `backend/crates/sober-core/src/telemetry.rs` | Modify: simplify signature, add spawn_metrics_server, add gRPC propagation |
| `backend/crates/sober-core/Cargo.toml` | Modify: add `tonic`, `tokio` deps |
| `backend/crates/sober-api/src/main.rs` | Modify: replace init_tracing, mount /metrics |
| `backend/crates/sober-api/src/state.rs` | Modify: add trace interceptor to agent client |
| `backend/crates/sober-api/src/middleware/` | Modify/Create: HTTP metrics middleware |
| `backend/crates/sober-api/src/routes/ws.rs` | Modify: add WebSocket metrics |
| `backend/crates/sober-agent/src/main.rs` | Modify: replace init_tracing, spawn metrics server |
| `backend/crates/sober-agent/src/agent.rs` | Modify: add agent loop + tool call metrics |
| `backend/crates/sober-scheduler/src/main.rs` | Modify: replace init_tracing, spawn metrics server |
| `backend/crates/sober-scheduler/src/engine.rs` | Modify: add tick + job execution metrics |
| `backend/crates/sober-llm/src/client.rs` | Modify: add LLM request/token metrics |
| `backend/crates/sober-auth/src/service.rs` | Modify: add auth attempt/session metrics |
| `backend/crates/sober-memory/src/store/memory_store.rs` | Modify: add search/store/prune metrics |
| `backend/crates/sober-mind/src/assembly.rs` | Modify: add prompt assembly metrics |
| `backend/crates/sober-mind/src/injection.rs` | Modify: add injection detection counter |
| `backend/crates/sober-crypto/src/keys.rs` | Modify: add sign/verify counters |
| `backend/crates/sober-crypto/src/envelope.rs` | Modify: add encrypt/decrypt counters |
| `backend/crates/sober-mcp/src/client.rs` | Modify: add tool call + connection metrics |
| `backend/crates/sober-sandbox/src/bwrap.rs` | Modify: add execution metrics |
| `backend/crates/sober-db/src/pool.rs` | Modify: add pool gauge metrics |
| `backend/crates/sober-web/src/main.rs` | Modify: replace init_tracing |
| `docker-compose.yml` | Modify: add prometheus, tempo, grafana |
| `justfile` | Modify: add dashboards, observability commands |
| `infra/prometheus/**` | Create: scrape config + alert rules |
| `infra/tempo/tempo.yml` | Create: Tempo config |
| `infra/grafana/**` | Create: provisioning + dashboards |
| `tools/dashboard-gen/**` | Create: standalone Rust binary |
| `backend/crates/*/metrics.toml` | Create: 13 metric definition files |

## Existing Code to Reuse

- `init_telemetry()` at `sober-core/src/telemetry.rs:68` — tracing + OTEL + Prometheus setup
- `MetricsEndpoint` at `sober-core/src/telemetry.rs:173` — axum IntoResponse for /metrics
- `TelemetryGuard` at `sober-core/src/telemetry.rs:40` — holds PrometheusHandle + OTEL provider
- `try_init_otel_tracing()` at `sober-core/src/telemetry.rs:125` — optional OTEL setup
- Design doc metrics at `docs/plans/pending/018-observability/design.md:114-300`
- Alert rules at `docs/plans/pending/018-observability/design.md:349-381`
- Dashboard layout at `docs/plans/pending/018-observability/design.md:304-340`
