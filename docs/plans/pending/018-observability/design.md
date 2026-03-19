# Observability, Metrics & Dashboards Design

**Date:** 2026-03-06
**Updated:** 2026-03-19
**Status:** Pending
**Scope:** Cross-cutting — all crates and services

## Goal

Comprehensive observability for the Sober system covering metrics, distributed tracing, structured logging, auto-generated dashboards, and alerting. Designed for both developer insight during development and operational monitoring in production.

## Architecture: Injection Detection in sober-mind

Injection detection lives in `sober-mind`, which owns prompt assembly and the
boundary between raw user input and assembled prompts (decided in C9).
`sober-crypto` handles only cryptographic operations.

Metric: `sober_mind_injection_detections_total`.

---

## Stack

| Component | Role | Notes |
|-----------|------|-------|
| `tracing` + `tracing-subscriber` | Structured logging (JSON in prod) | Already in `sober-core` |
| `tracing-opentelemetry` | Bridge tracing spans to OTEL | New dependency |
| `opentelemetry` + `opentelemetry-otlp` | Trace export to Tempo | New dependency |
| `metrics` + `metrics-exporter-prometheus` | In-process metric registry + `/metrics` endpoint | New dependency |
| Prometheus | Metric scraping & storage | Docker container |
| Grafana Tempo | Distributed trace storage | Docker container |
| Grafana | Dashboards, alerting | Docker container |

**No Loki/Promtail** — logs stay on stdout. All backends (Prometheus, Tempo) are optional consumers. The app writes logs to stdout, exposes `/metrics`, and emits OTEL spans regardless of whether anything collects them. Disabling OTEL export is a config change (unset `OTEL_EXPORTER_OTLP_ENDPOINT`).

---

## Approach: Instrumentation in `sober-core`, Metrics Defined Per-Crate

`sober-core` provides telemetry initialization and shared helpers. Each crate defines its own domain-specific metrics using shared primitives and conventions. Metrics live next to the code they measure.

### `sober-core` Responsibilities (implemented in plan 003)

`init_telemetry()` is implemented as part of plan 003 (sober-core), not this plan.
It replaces `init_tracing()` and provides:

  1. `tracing-subscriber` with JSON formatter (existing behavior)
  2. OTEL trace exporter layer pointing at Tempo (configurable endpoint, disabled if unset)
  3. Prometheus metrics recorder (always active — in-memory registry)

Additionally, sober-core exports:
- Helper functions for metric registration conventions
- Standard label constants: `service`, `method`, `status`, `crate`
- `MetricsEndpoint` axum handler that serves `/metrics` in Prometheus format

**Plan 018 scope:** This plan covers Docker infrastructure (Prometheus, Tempo,
Grafana), per-crate `metrics.toml` files, the dashboard generation tool, alerting rules,
and gRPC trace propagation interceptors. The core instrumentation API is already in place
from plan 003.

### gRPC Trace Propagation

Each tonic client gets an interceptor that injects W3C `traceparent` into gRPC metadata. Each tonic server gets a layer that extracts it and sets the parent span. This connects traces across `api -> agent`, `scheduler -> agent`, etc.

### Configuration

All telemetry config via env vars with sensible defaults:

| Variable | Purpose | Default |
|----------|---------|---------|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Tempo endpoint | Unset (disabled) |
| `OTEL_SERVICE_NAME` | Service identity in traces | Auto-set per binary |
| `METRICS_LISTEN_ADDR` | Where `/metrics` binds | Same as service port |
| `OTEL_TRACES_SAMPLER` | Sampling strategy | `always_on` |

**Sampling:** Start with 100% (`always_on`) for both dev and prod. Tune later
once we have data on trace volume and storage costs.

### Metric Naming Convention

```
sober_<crate>_<noun>_<unit>
```

Examples: `sober_api_request_duration_seconds`, `sober_llm_tokens_total`, `sober_memory_search_duration_seconds`.

Labels kept minimal and consistent: `method`, `path`, `status`, `provider`, `model`, `scope`, `job_type`, `tool`, `plugin`, `source`, `operation` — as appropriate per metric.

### Metric Registry Files

Each crate declares its metrics in a `metrics.toml` file at the crate root. This file serves as both documentation and input to the dashboard generation tool.

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

[[metrics]]
name = "sober_llm_request_duration_seconds"
type = "histogram"
help = "LLM request latency"
labels = ["provider", "model"]
group = "Requests"
buckets = [0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0]

  [[metrics.alerts]]
  name = "LLMLatencyDegraded"
  severity = "warning"
  expr = "histogram_quantile(0.95, rate({{name}}_bucket[5m])) > 15"
  for = "5m"
  summary = "LLM p95 latency above 15s"
```

#### Histogram Bucket Defaults

The dashboard-gen tool applies sensible defaults per metric suffix. `metrics.toml`
can override with an explicit `buckets` field:

| Suffix | Default buckets |
|--------|----------------|
| `_duration_seconds` | `[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]` |
| `_bytes` | `[256, 1024, 4096, 16384, 65536, 262144, 1048576, 4194304]` |
| `_count` / `_per_request` / `_per_tick` | `[1, 2, 5, 10, 20, 50, 100]` |

#### Per-Metric Alert Definitions

Metrics can optionally declare `[[metrics.alerts]]` sections. The dashboard-gen
tool emits these as Prometheus alerting rules to `infra/prometheus/alerts/generated/`.

Fields:
- `name` — alert name (must be unique across all metrics.toml files)
- `severity` — `critical` or `warning`
- `expr` — PromQL expression; `{{name}}` is replaced with the metric name
- `for` — duration string (e.g., `"5m"`)
- `summary` — human-readable description

Cross-metric alerts (e.g., `ServiceDown`) that don't belong to a single crate
are hand-written in `infra/prometheus/alerts/curated/`.

---

## Per-Subsystem Metrics

### `sober-api` — HTTP/WebSocket Gateway

**Request metrics:**
- `sober_api_request_duration_seconds` (histogram) — labels: `method`, `path`, `status`
- `sober_api_request_total` (counter) — labels: `method`, `path`, `status`
- `sober_api_request_body_bytes` (histogram) — request payload size
- `sober_api_response_body_bytes` (histogram) — response payload size
- `sober_api_requests_in_flight` (gauge) — currently processing

**WebSocket:**
- `sober_api_ws_connections_active` (gauge) — current open connections
- `sober_api_ws_connections_total` (counter) — labels: `status` (opened/closed/error)
- `sober_api_ws_messages_total` (counter) — labels: `direction` (inbound/outbound), `type` (text/binary)
- `sober_api_ws_message_duration_seconds` (histogram) — time to process a WS message

**Rate limiting:**
- `sober_api_rate_limit_hits_total` (counter) — labels: `path`, `rule`
- `sober_api_rate_limit_remaining` (gauge) — per-client bucket state

**Admin socket:**
- `sober_api_admin_commands_total` (counter) — labels: `command`, `status`

### `sober-auth` — Authentication & Authorization

**Authentication:**
- `sober_auth_attempts_total` (counter) — labels: `method` (password/oidc/passkey/hwtoken), `status` (success/failure/locked)
- `sober_auth_attempt_duration_seconds` (histogram) — labels: `method`
- `sober_auth_lockouts_total` (counter) — brute-force lockout triggers

**Sessions:**
- `sober_auth_sessions_active` (gauge) — current valid sessions
- `sober_auth_sessions_created_total` (counter)
- `sober_auth_sessions_expired_total` (counter)

**Authorization:**
- `sober_auth_permission_checks_total` (counter) — labels: `permission`, `result` (allowed/denied)

### `sober-agent` — Orchestration & Task Execution

**Agent loop:**
- `sober_agent_requests_total` (counter) — labels: `trigger` (api/scheduler/replica), `status`
- `sober_agent_request_duration_seconds` (histogram) — full request lifecycle
- `sober_agent_loop_iterations_total` (counter) — LLM reasoning loop steps per request
- `sober_agent_loop_iterations_per_request` (histogram) — distribution of loop depth

**Tool execution:**
- `sober_agent_tool_calls_total` (counter) — labels: `tool`, `status` (success/error/timeout)
- `sober_agent_tool_call_duration_seconds` (histogram) — labels: `tool`

**Replica management:**
- `sober_agent_replicas_active` (gauge)
- `sober_agent_replicas_spawned_total` (counter)
- `sober_agent_delegation_total` (counter) — labels: `status`

**Task queue:**
- `sober_agent_tasks_queued` (gauge) — current queue depth
- `sober_agent_tasks_completed_total` (counter) — labels: `priority`, `status`
- `sober_agent_task_wait_duration_seconds` (histogram) — time in queue before execution

### `sober-llm` — LLM Provider Abstraction

**Calls:**
- `sober_llm_request_total` (counter) — labels: `provider`, `model`, `status`
- `sober_llm_request_duration_seconds` (histogram) — labels: `provider`, `model`
- `sober_llm_time_to_first_token_seconds` (histogram) — labels: `provider`, `model`

**Tokens:**
- `sober_llm_tokens_input_total` (counter) — labels: `provider`, `model`
- `sober_llm_tokens_output_total` (counter) — labels: `provider`, `model`
- `sober_llm_tokens_per_request` (histogram) — labels: `provider`, `model`, `direction` (input/output)

**Cost:**
- `sober_llm_estimated_cost_dollars_total` (counter) — labels: `provider`, `model`

**Errors & retries:**
- `sober_llm_retries_total` (counter) — labels: `provider`, `reason` (rate_limit/timeout/server_error)
- `sober_llm_rate_limit_hits_total` (counter) — labels: `provider`

**Embeddings:**
- `sober_llm_embed_request_total` (counter) — labels: `provider`
- `sober_llm_embed_request_duration_seconds` (histogram)
- `sober_llm_embed_dimensions` (gauge) — vector size per model

### `sober-memory` — Vector Storage & BCF

**Search:**
- `sober_memory_search_total` (counter) — labels: `scope`, `search_type` (dense/sparse/hybrid)
- `sober_memory_search_duration_seconds` (histogram) — labels: `scope`, `search_type`
- `sober_memory_search_results_count` (histogram) — how many results returned

**Storage:**
- `sober_memory_chunks_stored_total` (counter) — labels: `chunk_type` (Fact/Conversation/Skill/etc), `scope`
- `sober_memory_chunks_total` (gauge) — current count per scope/type
- `sober_memory_storage_bytes` (gauge) — labels: `scope`

**Pruning:**
- `sober_memory_prune_runs_total` (counter)
- `sober_memory_prune_duration_seconds` (histogram)
- `sober_memory_pruned_chunks_total` (counter) — how many removed per run
- `sober_memory_importance_score_distribution` (histogram) — distribution of scores at prune time

**BCF operations:**
- `sober_memory_bcf_encode_duration_seconds` (histogram)
- `sober_memory_bcf_decode_duration_seconds` (histogram)
- `sober_memory_bcf_size_bytes` (histogram) — encoded container sizes

### `sober-scheduler` — Tick Engine

**Jobs:**
- `sober_scheduler_jobs_registered` (gauge) — labels: `type` (interval/cron), `persistence` (ephemeral/persistent)
- `sober_scheduler_job_executions_total` (counter) — labels: `job_name`, `status` (success/error/timeout)
- `sober_scheduler_job_duration_seconds` (histogram) — labels: `job_name`
- `sober_scheduler_job_lag_seconds` (histogram) — scheduled time vs actual start time

**Tick engine:**
- `sober_scheduler_ticks_total` (counter)
- `sober_scheduler_tick_duration_seconds` (histogram) — time to evaluate all due jobs
- `sober_scheduler_jobs_due_per_tick` (histogram) — how many jobs fire per tick

**State:**
- `sober_scheduler_paused` (gauge) — 0 or 1
- `sober_scheduler_missed_executions_total` (counter) — jobs that couldn't run (e.g., during pause)

### `sober-crypto` — Cryptographic Operations

- `sober_crypto_sign_total` (counter) — labels: `algorithm`
- `sober_crypto_sign_duration_seconds` (histogram)
- `sober_crypto_verify_total` (counter) — labels: `algorithm`, `result` (valid/invalid)
- `sober_crypto_encrypt_total` (counter)
- `sober_crypto_encrypt_duration_seconds` (histogram)
- `sober_crypto_decrypt_total` (counter)
- `sober_crypto_decrypt_duration_seconds` (histogram)
- `sober_crypto_keypair_generated_total` (counter)

### `sober-mind` — Prompt Assembly & Identity

- `sober_mind_prompt_assembly_duration_seconds` (histogram) — labels: `trigger` (api/scheduler/replica/admin)
- `sober_mind_prompt_token_estimate` (histogram) — assembled prompt size
- `sober_mind_soul_layers_loaded` (histogram) — how many layers resolved per assembly
- `sober_mind_trait_evolution_proposals_total` (counter) — labels: `target` (soul_layer/base_soul), `decision` (adopted/rejected/pending)
- `sober_mind_access_mask_resolutions_total` (counter) — labels: `trigger`
- `sober_mind_injection_detections_total` (counter) — prompt injection classifier hits

### `sober-plugin` — Plugin System

- `sober_plugin_installed` (gauge) — currently installed plugins
- `sober_plugin_executions_total` (counter) — labels: `plugin`, `status`
- `sober_plugin_execution_duration_seconds` (histogram) — labels: `plugin`
- `sober_plugin_audit_runs_total` (counter) — labels: `plugin`, `result` (pass/fail)
- `sober_plugin_sandbox_violations_total` (counter) — labels: `plugin`, `violation_type`

### `sober-skill` — Skill Discovery & Activation

**Discovery:**
- `sober_skill_scan_duration_seconds` (histogram) — full filesystem scan latency
- `sober_skill_cache_hits_total` (counter) — TTL cache hits (no rescan needed)
- `sober_skill_cache_misses_total` (counter) — cache expired, triggered rescan
- `sober_skill_discovered_total` (counter) — labels: `source` (user/workspace)
- `sober_skill_catalog_size` (gauge) — current number of skills in catalog

**Activation:**
- `sober_skill_activation_total` (counter) — labels: `status` (success/not_found/already_active/error)
- `sober_skill_activation_duration_seconds` (histogram) — file read + parse + state update

### `sober-workspace` — Blob Store, Snapshots, Git Ops

**Blob store:**
- `sober_workspace_blob_duration_seconds` (histogram) — labels: `operation` (store/retrieve/delete)
- `sober_workspace_blob_operations_total` (counter) — labels: `operation`, `status` (success/dedup/not_found/error)
- `sober_workspace_blob_bytes_total` (counter) — cumulative bytes written

**Snapshots:**
- `sober_workspace_snapshot_duration_seconds` (histogram) — labels: `operation` (create/restore/prune)
- `sober_workspace_snapshot_operations_total` (counter) — labels: `operation`, `status`
- `sober_workspace_snapshots_active` (gauge) — current snapshot count

**Git worktrees:**
- `sober_workspace_worktree_duration_seconds` (histogram) — labels: `operation` (create/remove)
- `sober_workspace_worktree_operations_total` (counter) — labels: `operation`, `status`

**Git push:**
- `sober_workspace_git_push_duration_seconds` (histogram) — labels: `status`

### `sober-mcp` — MCP Tool Interop

- `sober_mcp_tool_calls_total` (counter) — labels: `server`, `tool`, `status`
- `sober_mcp_tool_call_duration_seconds` (histogram) — labels: `server`, `tool`
- `sober_mcp_server_connections_active` (gauge) — labels: `server`
- `sober_mcp_server_reconnects_total` (counter) — labels: `server`

### `sober-sandbox` — Process Sandboxing

- `sober_sandbox_executions_total` (counter) — labels: `profile`, `status` (success/denied/timeout)
- `sober_sandbox_execution_duration_seconds` (histogram) — labels: `profile`
- `sober_sandbox_policy_violations_total` (counter) — labels: `profile`, `violation` (network/filesystem/syscall)
- `sober_sandbox_resource_usage_cpu_seconds` (histogram) — per-execution CPU time
- `sober_sandbox_resource_usage_memory_bytes` (histogram) — per-execution peak memory

### Infrastructure / Connection Pools

- `sober_pg_pool_connections_active` (gauge)
- `sober_pg_pool_connections_idle` (gauge)
- `sober_pg_pool_acquire_duration_seconds` (histogram)
- `sober_pg_pool_timeouts_total` (counter)
- `sober_qdrant_pool_connections_active` (gauge)
- `sober_redis_pool_connections_active` (gauge)
- `sober_redis_commands_total` (counter) — labels: `command`, `status`
- `sober_redis_command_duration_seconds` (histogram)

### Process-Level (all services)

- `sober_process_cpu_seconds_total` (counter)
- `sober_process_resident_memory_bytes` (gauge)
- `sober_process_open_fds` (gauge)
- `sober_process_tokio_tasks_active` (gauge)
- `sober_process_tokio_tasks_spawned_total` (counter)
- `sober_process_uptime_seconds` (gauge)

---

## Dashboards

### Auto-Generated Dashboards

A build tool at `tools/dashboard-gen/` reads `metrics.toml` files from each crate and generates Grafana dashboard JSON **and** Prometheus alerting rules. Panel type is derived from metric type:

| Metric type | Panel template |
|-------------|---------------|
| `_duration_seconds` (histogram) | Heatmap + line chart with p50/p95/p99 |
| `_total` (counter) | Rate panel (`rate()` over selected interval) |
| `_bytes` (histogram) | Heatmap + p50/p95/p99 with byte unit formatting |
| gauge | Current value stat + time series |
| `_in_flight` / `_active` / `_queued` (gauge) | Stat panel + time series |

Panels grouped by the `group` field in `metrics.toml`. Each crate produces one dashboard. Variable dropdowns generated from label definitions for filtering.

Dashboard output: `infra/grafana/dashboards/generated/`
Alert rule output: `infra/prometheus/alerts/generated/`

Regenerate with `just dashboards`.

### Curated Overview Dashboard

One hand-crafted dashboard — the "glance at it and know if things are fine" view. Lives in `infra/grafana/dashboards/curated/` (not overwritten by the generator).

**Row 1 — System Health:**
Service uptime (all 3 processes), error rate across all services, active WebSocket connections, request rate (req/s).

**Row 2 — Agent Performance:**
Agent request rate + latency (p95), LLM latency (p95) + tokens/min, tool call success rate, estimated LLM cost (rolling 24h).

**Row 3 — Memory & Storage:**
Vector search latency (p95), chunks stored (by type), pruning activity, connection pool utilization (PG, Qdrant, Redis).

**Row 4 — Scheduler:**
Jobs registered vs executing, job lag (scheduled vs actual), missed executions, tick duration.

**Row 5 — Security:**
Auth attempts (success vs failure), injection detections, sandbox violations, permission denials.

---

## Alerting Rules

Two sources of alert rules, both loaded by Prometheus:

### Generated (from `metrics.toml`)

Per-metric `[[metrics.alerts]]` sections are emitted by the dashboard-gen tool
to `infra/prometheus/alerts/generated/`. One YAML file per crate. Regenerated
by `just dashboards`.

### Curated (hand-written)

Cross-metric and infrastructure alerts that don't belong to a single crate live
in `infra/prometheus/alerts/curated/`. These are not overwritten by the generator.

#### Critical (immediate notification)

| Alert | Condition | For |
|-------|-----------|-----|
| `ServiceDown` | Any of api/scheduler/agent unreachable | 1m |
| `HighErrorRate` | 5xx responses > 5% of total request rate | 5m |
| `LLMProviderDown` | Error rate > 90% for a provider | 3m |
| `DatabaseConnectionExhausted` | `sober_pg_pool_connections_idle` = 0 sustained | 2m |
| `InjectionDetected` | `sober_mind_injection_detections_total` rate > 0 | 0m |
| `SandboxViolation` | `sober_sandbox_policy_violations_total` rate > 0 | 0m |

#### Warning (check when convenient)

| Alert | Condition | For |
|-------|-----------|-----|
| `HighP95Latency` | API request p95 > 5s | 5m |
| `LLMLatencyDegraded` | LLM request p95 > 15s | 5m |
| `SchedulerJobLag` | Job lag p95 > 60s | 5m |
| `HighMemoryUsage` | Process RSS > 80% of limit | 10m |
| `ConnectionPoolPressure` | Idle connections < 10% of pool size | 5m |
| `AuthFailureSpike` | Auth failure rate spikes 3x baseline | 5m |
| `PruningBacklog` | Chunk count growing while prune removes 0 | 30m |
| `MissedSchedulerJobs` | Missed executions rate > 0 | 5m |

Note: `HighP95Latency` and `LLMLatencyDegraded` can alternatively be defined as
`[[metrics.alerts]]` in their respective crate `metrics.toml` files. Prefer
metrics.toml when the alert maps 1:1 to a single metric; use curated for
cross-metric or infrastructure alerts.

### Informational (dashboard-only, no notification)

| Metric | Purpose |
|--------|---------|
| LLM cost trending | Daily/weekly cost tracking |
| Token usage by model | Spot model selection inefficiencies |
| Replica spawn rate | Understand agent workload patterns |
| Trait evolution frequency | Track self-modification activity |

---

## Docker Compose Additions

```yaml
prometheus:
  image: prom/prometheus
  volumes:
    - ./infra/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml
    - ./infra/prometheus/alerts/:/etc/prometheus/alerts/
  ports:
    - "9090:9090"

tempo:
  image: grafana/tempo
  ports:
    - "4317:4317"   # OTLP gRPC
    - "3200:3200"   # Tempo query

grafana:
  image: grafana/grafana
  volumes:
    - ./infra/grafana/provisioning/:/etc/grafana/provisioning/
    - ./infra/grafana/dashboards/:/var/lib/grafana/dashboards/
  ports:
    - "3001:3000"
  environment:
    - GF_AUTH_ANONYMOUS_ENABLED=true
    - GF_AUTH_ANONYMOUS_ORG_ROLE=Viewer
```

---

## File Layout

```
infra/
  grafana/
    provisioning/
      datasources/datasources.yml    # Prometheus, Tempo
      dashboards/dashboards.yml      # Auto-load from /dashboards/
    dashboards/
      generated/                     # Output of dashboard-gen tool
      curated/                       # Hand-crafted overview dashboard
  prometheus/
    prometheus.yml                   # Scrape config for api, scheduler, agent
    alerts/
      generated/                     # Output of dashboard-gen tool (from metrics.toml alerts)
      curated/
        critical.yml                 # Cross-metric critical alerts
        warning.yml                  # Cross-metric warning alerts
  tempo/
    tempo.yml

tools/
  dashboard-gen/
    Cargo.toml
    src/main.rs                      # Reads metrics.toml, outputs Grafana JSON + Prometheus alerts

backend/crates/
  sober-core/metrics.toml
  sober-api/metrics.toml
  sober-auth/metrics.toml
  sober-agent/metrics.toml
  sober-llm/metrics.toml
  sober-memory/metrics.toml
  sober-scheduler/metrics.toml
  sober-crypto/metrics.toml
  sober-mind/metrics.toml
  sober-plugin/metrics.toml
  sober-skill/metrics.toml
  sober-workspace/metrics.toml
  sober-mcp/metrics.toml
  sober-sandbox/metrics.toml
  sober-db/metrics.toml
```

---

## Acceptance Criteria

- [ ] `init_telemetry()` in `sober-core` configures tracing, OTEL export, and Prometheus recorder
- [ ] Each crate has a `metrics.toml` defining its metrics (15 files)
- [ ] gRPC trace propagation works across api/agent/scheduler boundaries
- [ ] `/metrics` endpoint serves Prometheus format from each service
- [ ] `tools/dashboard-gen` generates valid Grafana dashboard JSON from `metrics.toml` files
- [ ] `tools/dashboard-gen` generates Prometheus alert rule YAML from `[[metrics.alerts]]` sections
- [ ] Docker Compose includes Prometheus, Tempo, Grafana
- [ ] Grafana auto-loads generated + curated dashboards on startup
- [ ] Alerting rules (generated + curated) provisioned and functional in Prometheus
- [ ] `just dashboards` regenerates all dashboard JSON and alert rules
- [ ] System operates normally when Tempo/Prometheus are not running (graceful degradation)

## Resolved Decisions

- **Histogram bucket defaults:** Dashboard-gen tool applies sensible defaults per metric suffix; `metrics.toml` can override with explicit `buckets` field.
- **OTEL sampling in production:** Start with `always_on` (100%). Tune later once we have data on trace volume and storage costs.
- **Alert thresholds in metrics.toml:** Yes — per-metric alerts defined as `[[metrics.alerts]]` in `metrics.toml`, emitted to `infra/prometheus/alerts/generated/` by dashboard-gen. Cross-metric alerts stay hand-written in `infra/prometheus/alerts/curated/`.
