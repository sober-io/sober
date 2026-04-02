# System Observability

Add structured tracing, service-layer instrumentation, and metrics cleanup across
all backend crates: `sober-api`, `sober-core`, `sober-agent`, `sober-scheduler`,
and `sober-llm`.

## Current State

The observability stack has solid foundations:
- `TraceLayer::new_for_http()` with default configuration
- `X-Request-ID` generation and propagation via `SetRequestIdLayer`
- Prometheus metrics (request count, latency, in-flight, WebSocket, rate limiting)
- Optional OpenTelemetry OTLP export with W3C TraceContext propagation
- Structured JSON logging in production, pretty-print in development
- Extensive `metrics.toml` declarations across 15 crates

Key gaps:
- Zero `#[instrument]` attributes on any handler, service, or agent function
- No domain context (user_id, conversation_id) attached to trace spans
- Service layer invisible to tracing; `TraceLayer` uses defaults
- Agent core loop (turn, dispatch, tools) is a tracing black box
- Scheduler tick engine and job executors have no structured spans
- LLM streaming path has no observability
- 11 ghost metrics (declared but never emitted), 9 undocumented metrics
- sqlx query tracing suppressed by default filter strings

## Approach

Hybrid: a middleware injects shared cross-cutting context (user_id, request_id)
into spans created by `TraceLayer`, while `#[instrument]` on service/agent/scheduler
methods adds domain-specific context. Existing manual gRPC spans are kept for OTel
trace context propagation. Metrics cleanup ensures declared metrics match reality.

## Components

### 1. Customized `TraceLayer` (sober-api)

Replace `TraceLayer::new_for_http()` in `main.rs` with a configured version:

```rust
TraceLayer::new_for_http()
    .make_span_with(|request: &Request<Body>| {
        let matched_path = request
            .extensions()
            .get::<MatchedPath>()
            .map(|p| p.as_str().to_owned());

        info_span!(
            "http_request",
            http.method = %request.method(),
            http.route = matched_path.as_deref().unwrap_or(""),
            http.status_code = tracing::field::Empty,
            user.id = tracing::field::Empty,
            request.id = tracing::field::Empty,
            otel.status_code = tracing::field::Empty,
            error.type_ = tracing::field::Empty,
            error.message = tracing::field::Empty,
        )
    })
    .on_response(|response: &Response, latency: Duration, span: &Span| {
        let status = response.status().as_u16();
        span.record("http.status_code", status);

        if status >= 500 {
            error!(latency_ms = latency.as_millis(), "request failed");
        } else if status >= 400 {
            warn!(latency_ms = latency.as_millis(), "client error");
        } else {
            info!(latency_ms = latency.as_millis(), "request completed");
        }
    })
    .on_failure(|error: ServerErrorsFailureClass, latency: Duration, _span: &Span| {
        error!(%error, latency_ms = latency.as_millis(), "request error");
    })
```

Creates spans with semantic field names aligned to OpenTelemetry conventions,
and classifies responses by status code range for appropriate log levels.

### 2. Request Context Middleware (`RequestContextLayer`)

New middleware in `sober-api/src/middleware/request_context.rs`.

Runs after auth middleware, before handlers. Reads `AuthUser` from request
extensions and `X-Request-ID` from headers, then records them on the current
span via `Span::current().record(...)`.

Fields injected:
- `user.id` -- from `AuthUser` extension (empty for unauthenticated routes)
- `request.id` -- from `X-Request-ID` header

Recording-only middleware -- creates no new spans, fills fields declared by
the customized `TraceLayer`.

### 3. Error Span Recording (sober-core)

Add span recording in `AppError`'s `IntoResponse` implementation.

When an `AppError` is converted to an HTTP response, record `otel.status_code`,
`error.type_`, and `error.message` on the current span. Every error response
is captured in the trace tree without explicit error logging per handler.

```rust
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let span = tracing::Span::current();
        span.record("otel.status_code", "ERROR");
        span.record("error.type_", error_type);
        span.record("error.message", &tracing::field::display(&self));
        // ... existing response construction
    }
}
```

### 4. API Service Layer Instrumentation

Add `#[instrument]` to all public service methods across all 11 service files
in `sober-api/src/services/`.

Rules:
- `skip(self)` always -- the pool/clients aren't useful in traces
- `skip` large input structs -- only record IDs
- Record domain IDs: `conversation.id`, `message.id`, `user.id`, `workspace.id`
- Level: `level = "info"` for mutations, `level = "debug"` for reads

Service files:
- `conversation.rs`, `message.rs`, `auth.rs`, `user.rs`, `attachment.rs`
- `collaborator.rs`, `evolution.rs`, `plugin.rs`, `tag.rs`
- `ws_dispatch.rs`, `verify_membership.rs`

### 5. sober-web Proxy Instrumentation

Add `#[instrument]` to the HTTP reverse proxy and WebSocket proxy handlers.
Record `upstream.path` on the HTTP proxy span.

### 6. Agent Instrumentation (sober-agent)

Comprehensive `#[instrument]` across all public methods (~40 functions).

**gRPC handlers** (`grpc/agent.rs`): Keep existing manual spans for OTel trace
context propagation. Normalize field names to system convention (`conversation.id`,
`user.id`, etc.).

**Actor model:**
- `Agent::handle_message()` -- fields: `conversation.id`, `user.id`
- `Agent::resolve_workspace_dir()`, `resolve_delivery_conversation()`
- `ConversationActor::run()`, `handle_message()`, `recover_incomplete_executions()`

**Core agentic loop:**
- `run_turn()` -- fields: `conversation.id`, `iteration`
- `build_context()`, `try_resolve_dynamic_engine()`, `load_attachment_data()`,
  `auto_generate_title()`
- `execute_tool_calls()` -- fields: `tool_count`

**Tool implementations:** `#[instrument]` on every tool's `execute()` method
(shell, memory, artifacts, scheduler, secrets, propose_*, etc.).

**Evolution:**
- `execute_evolution()` -- fields: `event.id`, `evolution.type`
- Type-specific executors (plugin, skill, instruction, automation)

**Ingestion pipeline:** memory extraction, embedding, storage functions.

**Spawn instrumentation:** Add `.instrument(span)` to `tokio::spawn` for
conversation actors, connecting child spans to the parent trace.

### 7. Scheduler Instrumentation (sober-scheduler)

Comprehensive `#[instrument]` across all public methods.

**Tick engine:**
- `TickEngine::tick()` -- `debug` level for the tick itself; meaningful span
  created conditionally only when `due_job_count > 0`, wrapping the job
  execution loop. Avoids noise from idle ticks.
- `route_job()` -- fields: `job.id`, `job.type`

**All 6 job executors** -- `execute()` on each:
- `ArtifactExecutor`, `MemoryPruningExecutor`, `SessionCleanupExecutor`,
  `BlobGcExecutor`, `AttachmentCleanupExecutor`, `PluginCleanupExecutor`

**gRPC service:**
- `pause()`, `resume()`, `force_run()`, `list_jobs()`, `get_job()`, `cancel_job()`

**Spawn instrumentation:** `.instrument(span)` on per-job `tokio::spawn` calls.

### 8. LLM Instrumentation (sober-llm)

`#[instrument]` on all public methods:

- `OpenAiCompatibleEngine::stream()` -- fields: `model`, `provider`
- `OpenAiCompatibleEngine::complete()` -- fields: `model`, `provider`
- `OpenAiCompatibleEngine::embed()` -- fields: `model`
- `AcpEngine::stream()` -- fields: `model`
- SSE parsing functions -- `debug` level

### 9. Metrics Cleanup

**Wire 4 ghost agent metrics:**
- `sober_agent_requests_total` -- emit in `Agent::handle_message()`
- `sober_agent_request_duration_seconds` -- emit in `Agent::handle_message()`
- `sober_agent_tool_calls_total` -- emit in `execute_tool_calls()`
- `sober_agent_tool_call_duration_seconds` -- emit in `execute_tool_calls()`

**Remove 7 ghost metrics from `metrics.toml`:**
- `sober-core`: 4 process metrics (`cpu_seconds`, `resident_memory`, `open_fds`,
  `uptime`)
- `sober-plugin`: 3 metrics (`installed` gauge, `audit_runs_total`,
  `sandbox_violations_total`)

**Document 9 undocumented metrics** -- add to respective `metrics.toml`:
- `sober-api`: 3 attachment metrics (`uploads_total`, `upload_bytes`,
  `upload_duration_seconds`)
- `sober-scheduler`: 4 metrics (`blob_gc_runs_total`, `blob_gc_deleted_total`,
  `blob_gc_bytes_freed_total`, `attachment_cleanup_deleted_total`)
- `sober-db`: `sober_message_content_blocks_total`
- `sober-workspace`: `sober_attachment_image_processing_seconds`
- `sober-agent`: `sober_llm_vision_blocks_resolved_total`

### 10. Database Query Tracing

Add `sqlx::query=warn` to default filter strings in all 4 binaries:
- `sober-api`, `sober-agent`, `sober-scheduler`, `sober-web`

Zero code changes beyond the filter string -- sqlx already emits tracing events
at `DEBUG` level. The `warn` filter surfaces only failed or slow queries.

## Instrumentation Rules (System-Wide)

- `skip(self)` always
- `skip` large payloads (content blocks, binary data, tool outputs)
- `level = "info"` for mutations (create, update, delete)
- `level = "debug"` for reads (list, get)
- Domain IDs as span fields: `conversation.id`, `message.id`, `user.id`,
  `job.id`, `event.id`, `tool.name`
- OpenTelemetry field naming conventions

## Structured Log Events at Key Decision Points

Beyond `#[instrument]` spans, the critical request paths need explicit log
events (`tracing::info!`, `debug!`, `warn!`) with structured metadata at
important decision points, state transitions, and lifecycle boundaries.
`#[instrument]` tells you a function was entered/exited; log events tell you
what happened inside.

### Agent Core Loop (`turn.rs`, `dispatch.rs`)

- **Context loading**: message count, memory chunk count, system prompt length
- **LLM stream start**: model, tool count, history message count
- **LLM stream end**: content length, tool call count and names
- **Tool dispatch start**: tool count and names
- **Per-tool completion**: tool name, is_error, output length
- **Dispatch outcome**: any_context_modifying, any_errors flags
- **Confirmation flow**: request registered, response received (approved/denied)

### Conversation Actor (`conversation.rs`)

- **Agent mode resolved**: which mode (silent/mention/always)
- **Injection verdict**: rejected verdicts (warn level)
- **Workspace resolution**: dir, has_settings
- **Tool registry built**: tool count
- **Skills loaded**: skill count
- **Crash recovery**: count of recovered executions

### LLM Client (`client.rs`)

- **Stream/complete request sent**: model, provider, max_tokens, tool count
- **HTTP error response**: status code, model
- **Embedding request**: model, text count

### Scheduler Engine (`engine.rs`)

- **Job routing**: job ID/name, route (agent vs local)
- **Agent gRPC call**: before/after with job ID
- **Agent client connect/disconnect**: state transitions

## Middleware Stack Order (sober-api)

The layer order in `main.rs` (outermost first, last in `.layer()` chain):

```
PropagateRequestIdLayer   -- propagate X-Request-ID to response
SetRequestIdLayer         -- generate X-Request-ID
TraceLayer (customized)   -- create http_request span with empty fields
RequestContextLayer       -- fill user.id and request.id on current span
CORS                      -- CORS headers
RateLimitLayer            -- rate limiting
HttpMetricsLayer          -- Prometheus counters
[Router + AuthLayer]      -- route matching + auth
```

`RequestContextLayer` must run after `SetRequestIdLayer` (so the header exists)
and after `AuthLayer` (so `AuthUser` is in extensions). Since layers execute
inside-out, `RequestContextLayer` goes before the others in the `.layer()` chain.

## What This Does NOT Include

- Request/response body logging (security risk, unnecessary overhead)
- New Prometheus metric definitions beyond wiring existing ghost metrics
- Handler-level `#[instrument]` (TraceLayer span + service span is sufficient)
- Database query-level Prometheus metrics (separate effort if needed)
- Custom histogram buckets for sqlx query timing
