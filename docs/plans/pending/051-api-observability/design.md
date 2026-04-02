# API Observability Improvement

Improve observability in `sober-api` and `sober-core` telemetry by adding structured
request context, service-layer instrumentation, and richer trace spans.

## Current State

The observability stack has solid foundations:
- `TraceLayer::new_for_http()` with default configuration
- `X-Request-ID` generation and propagation via `SetRequestIdLayer`
- Prometheus metrics (request count, latency, in-flight, WebSocket, rate limiting)
- Optional OpenTelemetry OTLP export with W3C TraceContext propagation
- Structured JSON logging in production, pretty-print in development

Key gaps: zero `#[instrument]` attributes on any handler or service, no domain
context (user_id, conversation_id) attached to trace spans, service layer is
invisible to tracing, and `TraceLayer` uses defaults with no response classification.

## Approach

Hybrid: a middleware injects shared cross-cutting context (user_id, request_id)
into spans created by `TraceLayer`, while `#[instrument]` on service methods adds
domain-specific context (conversation_id, message_id, etc.).

## Components

### 1. Request Context Middleware (`RequestContextLayer`)

New middleware in `sober-api/src/middleware/request_context.rs`.

Runs after auth middleware, before handlers. Reads `AuthUser` from request
extensions and `X-Request-ID` from headers, then records them on the current
span via `Span::current().record(...)`.

Fields injected:
- `user.id` -- from `AuthUser` extension (empty for unauthenticated routes)
- `request.id` -- from `X-Request-ID` header

This is a recording-only middleware -- it creates no new spans, just fills in
fields that the customized `TraceLayer` span declares as empty.

### 2. Customized `TraceLayer`

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

This creates spans with semantic field names aligned to OpenTelemetry conventions,
and classifies responses by status code range for appropriate log levels.

### 3. Service Layer Instrumentation

Add `#[instrument]` to all public service methods across all 11 service files.

Pattern:
```rust
#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn get(&self, conversation_id: ConversationId, user_id: UserId) -> Result<...> {
```

Rules:
- `skip(self)` always -- the pool/clients aren't useful in traces
- `skip` large input structs (e.g., `UpdateSettingsInput`) -- only record IDs
- Record domain IDs relevant to the operation: `conversation.id`, `message.id`,
  `user.id`, `workspace.id`
- Level: `level = "info"` for mutations (create, update, delete), `level = "debug"` for reads (list, get)

Service files to instrument:
- `conversation.rs` (~11 methods)
- `message.rs`
- `auth.rs`
- `user.rs`
- `attachment.rs`
- `collaborator.rs`
- `evolution.rs`
- `plugin.rs`
- `tag.rs`
- `ws_dispatch.rs` (already partially instrumented -- extend)
- `verify_membership.rs`

### 4. Error Span Recording

Add span recording in `AppError`'s `IntoResponse` implementation (in `sober-core`).

When an `AppError` is converted to an HTTP response, record `error.type` (the
variant name) and `error.message` on the current span. This ensures every error
response is captured in the trace tree without requiring each handler to log
errors explicitly.

```rust
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let span = Span::current();
        span.record("otel.status_code", "ERROR");
        span.record("error.type", self.code());
        span.record("error.message", &self.to_string());
        // ... existing response construction
    }
}
```

Note: The span must have these fields declared as `Empty` in `make_span_with` for
recording to work. Since `AppError` responses flow through the `TraceLayer` span,
we add `otel.status_code`, `error.type`, and `error.message` as empty fields in
the custom `make_span_with` closure.

### 5. sober-web Proxy Instrumentation

Add `#[instrument]` to the two proxy handler functions in `sober-web/src/main.rs`:

- HTTP reverse proxy handler: record `upstream.path`
- WebSocket proxy handler: record `upstream.path`

These are the only two functions that need instrumentation in sober-web; the
`TraceLayer` already handles request-level spans.

## Middleware Stack Order

The layer order in `main.rs` (outermost first, meaning last in `.layer()` chain):

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
- New Prometheus metrics or metrics.toml changes
- Database query timing (sqlx has its own tracing integration if enabled)
- Handler-level `#[instrument]` (the TraceLayer span + service span is sufficient)
