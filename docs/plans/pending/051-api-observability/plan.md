# #051 System Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add structured tracing (`#[instrument]`), request context propagation, metrics cleanup, and database query tracing across all backend crates so every request produces a rich trace tree with user/domain context.

**Architecture:** A custom `TraceLayer` creates HTTP-level spans with empty fields. A `RequestContextLayer` fills those fields from auth state. All service/agent/scheduler/LLM methods get `#[instrument]` for domain-specific spans. `AppError` records error details on spans. Metrics cleanup wires ghost metrics and documents undocumented ones. sqlx query tracing enabled via filter strings.

**Tech Stack:** `tracing` (instrument macro), `tower-http` (TraceLayer customization), `tower` (Layer/Service traits), `metrics` crate (Prometheus counters/histograms)

---

### Task 1: Customize `TraceLayer` in `sober-api`

Replace the default `TraceLayer::new_for_http()` with a configured version that creates spans with semantic field names and classifies responses by status code.

**Files:**
- Modify: `backend/crates/sober-api/src/main.rs:58-63`

- [ ] **Step 1: Update imports in main.rs**

Add the necessary imports for the customized TraceLayer. Replace the existing import block at the top:

```rust
use std::net::SocketAddr;
use std::time::Duration;

use axum::extract::MatchedPath;
use axum::routing::get;
use axum_core::body::Body;
use http::Method;
use http::Response;
use http::header::{AUTHORIZATION, CONTENT_TYPE};
use sober_api::admin;
use sober_api::middleware::metrics::HttpMetricsLayer;
use sober_api::middleware::rate_limit::{RateLimitConfig, RateLimitLayer};
use sober_api::routes;
use sober_api::state::AppState;
use sober_core::MetricsEndpoint;
use sober_core::config::{AppConfig, Environment};
use tokio::net::TcpListener;
use tokio::signal;
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use tracing::{info, info_span, Span};
```

- [ ] **Step 2: Replace TraceLayer with customized version**

Replace lines 58-63 (the `let app = app` block) with:

```rust
let app = app
    .layer(cors)
    .layer(rate_limit)
    .layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &http::Request<Body>| {
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
            .on_response(
                |response: &Response<Body>, latency: Duration, span: &Span| {
                    let status = response.status().as_u16();
                    span.record("http.status_code", status);

                    if status >= 500 {
                        tracing::error!(
                            latency_ms = latency.as_millis() as u64,
                            "request failed"
                        );
                    } else if status >= 400 {
                        tracing::warn!(
                            latency_ms = latency.as_millis() as u64,
                            "client error"
                        );
                    } else {
                        tracing::info!(
                            latency_ms = latency.as_millis() as u64,
                            "request completed"
                        );
                    }
                },
            )
            .on_failure(
                |error: tower_http::classify::ServerErrorsFailureClass,
                 latency: Duration,
                 _span: &Span| {
                    tracing::error!(
                        %error,
                        latency_ms = latency.as_millis() as u64,
                        "request error"
                    );
                },
            ),
    )
    .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
    .layer(PropagateRequestIdLayer::x_request_id());
```

- [ ] **Step 3: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-api/src/main.rs
git commit -m "feat(api): customize TraceLayer with semantic span fields and response classification"
```

---

### Task 2: Create `RequestContextLayer` middleware

A recording-only middleware that fills `user.id` and `request.id` on the current span from request extensions and headers.

**Files:**
- Create: `backend/crates/sober-api/src/middleware/request_context.rs`
- Modify: `backend/crates/sober-api/src/middleware/mod.rs`
- Modify: `backend/crates/sober-api/src/routes/mod.rs`

- [ ] **Step 1: Create the middleware module**

Create `backend/crates/sober-api/src/middleware/request_context.rs`:

```rust
//! Request context middleware.
//!
//! Records `user.id` and `request.id` on the current tracing span from
//! request extensions and headers. Does not create new spans — only fills
//! in empty fields declared by the `TraceLayer` span.

use std::task::{Context, Poll};

use axum_core::body::Body;
use http::Request;
use sober_auth::AuthUser;
use tower::{Layer, Service};
use tracing::Span;

/// Tower [`Layer`] that records request context on the current span.
#[derive(Clone, Default)]
pub struct RequestContextLayer;

impl RequestContextLayer {
    /// Creates a new request context layer.
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

/// Middleware service that records user and request identity on the current span.
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
```

- [ ] **Step 2: Register the module**

In `backend/crates/sober-api/src/middleware/mod.rs`, add:

```rust
pub mod request_context;
```

- [ ] **Step 3: Wire into the router**

In `backend/crates/sober-api/src/routes/mod.rs`, add the import and layer. Replace the `build_router` function:

```rust
use crate::middleware::request_context::RequestContextLayer;

/// Builds the complete API router with all routes and middleware.
pub fn build_router(state: Arc<AppState>) -> Router {
    let auth_layer = AuthLayer::<PgUserRepo, PgSessionRepo, PgRoleRepo>::new(state.auth.clone());

    let api = Router::new()
        .merge(health::routes())
        .merge(system::routes())
        .merge(attachments::routes())
        .merge(auth::routes())
        .merge(conversations::routes())
        .merge(collaborators::routes())
        .merge(evolution::routes())
        .merge(messages::routes())
        .merge(plugins::routes())
        .merge(tags::routes())
        .merge(users::routes())
        .merge(workspaces::routes())
        .merge(ws::routes())
        .layer(RequestContextLayer::new())
        .layer(auth_layer)
        .with_state(state);

    Router::new().nest("/api/v1", api)
}
```

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-api/src/middleware/request_context.rs \
       backend/crates/sober-api/src/middleware/mod.rs \
       backend/crates/sober-api/src/routes/mod.rs
git commit -m "feat(api): add RequestContextLayer to record user.id and request.id on spans"
```

---

### Task 3: Add error span recording to `AppError`

Record error details on the current span when an `AppError` is converted to an HTTP response.

**Files:**
- Modify: `backend/crates/sober-core/src/error.rs:59-78`

- [ ] **Step 1: Update the `IntoResponse` implementation**

Replace the `IntoResponse` impl (lines 59-78) with:

```rust
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_type) = match &self {
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Validation(_) => (StatusCode::BAD_REQUEST, "validation_error"),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };

        let span = tracing::Span::current();
        span.record("otel.status_code", "ERROR");
        span.record("error.type_", error_type);
        span.record("error.message", tracing::field::display(&self));

        let body = ApiErrorEnvelope {
            error: ApiErrorBody {
                code: error_type.to_owned(),
                message: self.to_string(),
            },
        };

        (status, Json(body)).into_response()
    }
}
```

- [ ] **Step 2: Add tracing import**

Add at the top of `error.rs`, after the existing imports:

```rust
// tracing is used in IntoResponse to record error fields on the current span.
```

No explicit `use tracing;` needed — `tracing::Span` and `tracing::field` are used fully qualified.

- [ ] **Step 3: Build and run existing tests**

Run: `cd backend && cargo build -p sober-core -q && cargo test -p sober-core -q`
Expected: All existing error tests pass. Span recording is a no-op when there is no active span.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-core/src/error.rs
git commit -m "feat(core): record error details on current tracing span in AppError::into_response"
```

---

### Task 4: Instrument API services — `ConversationService`

Add `#[instrument]` to all public methods on `ConversationService`.

**Files:**
- Modify: `backend/crates/sober-api/src/services/conversation.rs`

- [ ] **Step 1: Add tracing import and instrument all methods**

Add `use tracing::instrument;` at the top of the file, then add these attributes before each method:

```rust
// Line ~115:
#[instrument(level = "debug", skip(self))]
pub async fn list(...)

// Line ~125:
#[instrument(skip(self))]
pub async fn create(...)

// Line ~172:
#[instrument(level = "debug", skip(self), fields(conversation.id = %conversation_id))]
pub async fn get(...)

// Line ~209:
#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn update(...)

// Line ~239:
#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn delete(...)

// Line ~253:
#[instrument(level = "debug", skip(self), fields(conversation.id = %conversation_id))]
pub async fn get_settings(...)

// Line ~273:
#[instrument(skip(self, input), fields(conversation.id = %conversation_id))]
pub async fn update_settings(...)

// Line ~348:
#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn mark_read(...)

// Line ~377:
#[instrument(level = "debug", skip(self))]
pub async fn get_inbox(...)

// Line ~395:
#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn convert_to_group(...)

// Line ~436:
#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn clear_messages(...)

// Line ~459:
#[instrument(level = "debug", skip(self), fields(conversation.id = %conversation_id))]
pub async fn list_jobs(...)
```

- [ ] **Step 2: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-api/src/services/conversation.rs
git commit -m "feat(api): instrument ConversationService with tracing spans"
```

---

### Task 5: Instrument API services — `MessageService`, `AuthService`, `UserService`

**Files:**
- Modify: `backend/crates/sober-api/src/services/message.rs`
- Modify: `backend/crates/sober-api/src/services/auth.rs`
- Modify: `backend/crates/sober-api/src/services/user.rs`

- [ ] **Step 1: Instrument MessageService**

Add `use tracing::instrument;` at the top of `message.rs`, then:

```rust
// Line ~36:
#[instrument(level = "debug", skip(self), fields(conversation.id = %conversation_id))]
pub async fn list(...)

// Line ~133:
#[instrument(skip(self), fields(message.id = %message_id))]
pub async fn delete(...)
```

- [ ] **Step 2: Instrument AuthService**

Add `use tracing::instrument;` at the top of `auth.rs`, then:

```rust
// Line ~27:
#[instrument(skip(self))]
pub async fn create_inbox_for_user(...)

// Line ~34:
#[instrument(level = "debug", skip(self))]
pub async fn get_user_with_roles(...)
```

- [ ] **Step 3: Instrument UserService**

Add `use tracing::instrument;` at the top of `user.rs`, then:

```rust
// Line ~24:
#[instrument(level = "debug", skip(self))]
pub async fn search(...)
```

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-api/src/services/message.rs \
       backend/crates/sober-api/src/services/auth.rs \
       backend/crates/sober-api/src/services/user.rs
git commit -m "feat(api): instrument MessageService, AuthService, UserService"
```

---

### Task 6: Instrument API services — `AttachmentService`, `CollaboratorService`

**Files:**
- Modify: `backend/crates/sober-api/src/services/attachment.rs`
- Modify: `backend/crates/sober-api/src/services/collaborator.rs`

- [ ] **Step 1: Instrument AttachmentService**

Add `use tracing::instrument;` at the top of `attachment.rs`, then:

```rust
// Line ~27:
#[instrument(skip(self, data), fields(conversation.id = %conversation_id, attachment.filename = %filename))]
pub async fn upload(...)
```

- [ ] **Step 2: Instrument CollaboratorService**

Add `use tracing::instrument;` at the top of `collaborator.rs`, then:

```rust
// Line ~28:
#[instrument(level = "debug", skip(self), fields(conversation.id = %conversation_id))]
pub async fn list(...)

// Line ~39:
#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn add(...)

// Line ~126:
#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn update_role(...)

// Line ~198:
#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn remove(...)

// Line ~266:
#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn leave(...)
```

- [ ] **Step 3: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-api/src/services/attachment.rs \
       backend/crates/sober-api/src/services/collaborator.rs
git commit -m "feat(api): instrument AttachmentService and CollaboratorService"
```

---

### Task 7: Instrument API services — `EvolutionService`, `PluginService`

**Files:**
- Modify: `backend/crates/sober-api/src/services/evolution.rs`
- Modify: `backend/crates/sober-api/src/services/plugin.rs`

- [ ] **Step 1: Instrument EvolutionService**

Add `use tracing::instrument;` at the top of `evolution.rs`, then:

```rust
// Line ~51:
#[instrument(level = "debug", skip(self))]
pub async fn list_events(...)

// Line ~61:
#[instrument(level = "debug", skip(self), fields(evolution.id = %id))]
pub async fn get_event(...)

// Line ~67:
#[instrument(skip(self), fields(evolution.id = %id))]
pub async fn update_event(...)

// Line ~108:
#[instrument(level = "debug", skip(self))]
pub async fn get_config(...)

// Line ~122:
#[instrument(skip(self, input))]
pub async fn update_config(...)

// Line ~153:
#[instrument(level = "debug", skip(self))]
pub async fn get_timeline(...)
```

- [ ] **Step 2: Instrument PluginService**

Add `use tracing::instrument;` at the top of `plugin.rs`, then:

```rust
// Line ~104:
#[instrument(level = "debug", skip(self))]
pub async fn list(...)

// Line ~129:
#[instrument(skip(self, config))]
pub async fn install(...)

// Line ~156:
#[instrument(skip(self, mcp_servers))]
pub async fn import(...)

// Line ~174:
#[instrument(skip(self))]
pub async fn reload(...)

// Line ~188:
#[instrument(level = "debug", skip(self), fields(plugin.id = %id))]
pub async fn get(...)

// Line ~210:
#[instrument(skip(self, config), fields(plugin.id = %id))]
pub async fn update(...)

// Line ~278:
#[instrument(skip(self), fields(plugin.id = %id))]
pub async fn uninstall(...)

// Line ~294:
#[instrument(level = "debug", skip(self), fields(plugin.id = %id))]
pub async fn list_audit_logs(...)

// Line ~327:
#[instrument(level = "debug", skip(self))]
pub async fn list_skills(...)

// Line ~353:
#[instrument(skip(self))]
pub async fn reload_skills(...)

// Line ~379:
#[instrument(level = "debug", skip(self))]
pub async fn list_tools(...)
```

- [ ] **Step 3: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-api/src/services/evolution.rs \
       backend/crates/sober-api/src/services/plugin.rs
git commit -m "feat(api): instrument EvolutionService and PluginService"
```

---

### Task 8: Instrument API services — `TagService`, `WsDispatchService`, `verify_membership`

**Files:**
- Modify: `backend/crates/sober-api/src/services/tag.rs`
- Modify: `backend/crates/sober-api/src/services/ws_dispatch.rs`
- Modify: `backend/crates/sober-api/src/services/verify_membership.rs`

- [ ] **Step 1: Instrument TagService**

Add `use tracing::instrument;` at the top of `tag.rs`, then:

```rust
// Line ~18:
#[instrument(level = "debug", skip(self))]
pub async fn list_by_user(...)

// Line ~24:
#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn add_to_conversation(...)

// Line ~38:
#[instrument(skip(self), fields(conversation.id = %conversation_id, tag.id = %tag_id))]
pub async fn remove_from_conversation(...)

// Line ~51:
#[instrument(skip(self), fields(message.id = %message_id))]
pub async fn add_to_message(...)

// Line ~68:
#[instrument(skip(self), fields(message.id = %message_id, tag.id = %tag_id))]
pub async fn remove_from_message(...)
```

- [ ] **Step 2: Instrument WsDispatchService**

Add `use tracing::instrument;` at the top of `ws_dispatch.rs`, then:

```rust
// Line ~29:
#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn subscribe(...)

// Line ~52:
#[instrument(skip(self, content, error_tx), fields(conversation.id = %conversation_id))]
pub async fn send_message(...)

// Line ~147:
#[instrument(skip(self))]
pub async fn confirm_response(...)

// Line ~164:
#[instrument(skip(self))]
pub async fn set_permission_mode(...)
```

- [ ] **Step 3: Instrument verify_membership**

Add `use tracing::instrument;` at the top of `verify_membership.rs`, then:

```rust
// Line ~7:
#[instrument(level = "debug", skip(db), fields(conversation.id = %conversation_id))]
pub(crate) async fn verify_membership(...)
```

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-api/src/services/tag.rs \
       backend/crates/sober-api/src/services/ws_dispatch.rs \
       backend/crates/sober-api/src/services/verify_membership.rs
git commit -m "feat(api): instrument TagService, WsDispatchService, verify_membership"
```

---

### Task 9: Instrument `sober-web` proxy handlers

**Files:**
- Modify: `backend/crates/sober-web/src/main.rs:134-162`

- [ ] **Step 1: Add tracing import and instrument proxy handlers**

Add `use tracing::instrument;` to the imports in `main.rs`, then add attributes:

```rust
// Line ~134:
#[instrument(skip_all, fields(upstream.path = %original_uri.path()))]
async fn reverse_proxy(
    State(state): State<ProxyState>,
    original_uri: axum::extract::OriginalUri,
    mut req: Request<Body>,
) -> Result<Response, StatusCode> {

// Line ~158:
#[instrument(skip_all)]
async fn ws_reverse_proxy(
    State(state): State<ProxyState>,
    headers: http::HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
```

- [ ] **Step 2: Build and verify**

Run: `cd backend && cargo build -p sober-web -q`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-web/src/main.rs
git commit -m "feat(web): instrument reverse proxy handlers with tracing spans"
```

---

### Task 10: Instrument `sober-agent` — Actor model

Add `#[instrument]` to `Agent` methods and `ConversationActor` methods.

**Files:**
- Modify: `backend/crates/sober-agent/src/agent.rs`
- Modify: `backend/crates/sober-agent/src/conversation.rs`

- [ ] **Step 1: Instrument Agent methods**

Add `use tracing::instrument;` at the top of `agent.rs`, then:

```rust
// Line ~251:
#[instrument(level = "debug", skip(self), fields(conversation.id = %conversation_id))]
pub async fn resolve_workspace_dir(...)

// Line ~303:
#[instrument(level = "debug", skip(self))]
pub async fn resolve_delivery_conversation(...)

// Line ~384:
#[instrument(skip(self, content), fields(conversation.id = %conversation_id, trigger = ?trigger))]
pub async fn handle_message(...)

// Line ~482:
#[instrument(skip(self))]
pub async fn shutdown(...)
```

- [ ] **Step 2: Add `.instrument(span)` to conversation actor spawn**

Find the `tokio::spawn` call at line ~438 in `agent.rs` that spawns a `ConversationActor`. Wrap it with a span:

```rust
let actor_span = tracing::info_span!(
    "conversation_actor",
    conversation.id = %conversation_id,
);
tokio::spawn(actor.run().instrument(actor_span));
```

Add `use tracing::Instrument;` to the imports if not present.

- [ ] **Step 3: Instrument ConversationActor methods**

Add `use tracing::instrument;` at the top of `conversation.rs`, then:

```rust
// Line ~110:
#[instrument(skip_all, fields(conversation.id = %self.conversation_id))]
pub async fn run(...)

// Line ~172:
#[instrument(skip(self, content, event_tx), fields(trigger = ?trigger))]
async fn handle_message(...)

// Line ~223:
#[instrument(skip(self, content, event_tx), fields(trigger = ?trigger))]
async fn handle_message_inner(...)

// Line ~368:
#[instrument(skip(self, conversation))]
async fn ensure_workspace(...)

// Line ~447:
#[instrument(skip(self))]
async fn recover_incomplete_executions(...)
```

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-agent -q`
Expected: Compiles without errors.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-agent/src/agent.rs \
       backend/crates/sober-agent/src/conversation.rs
git commit -m "feat(agent): instrument Actor model and ConversationActor with tracing spans"
```

---

### Task 11: Instrument `sober-agent` — Core agentic loop

Add `#[instrument]` to `run_turn`, `build_context`, and related functions in `turn.rs` and `dispatch.rs`.

**Files:**
- Modify: `backend/crates/sober-agent/src/turn.rs`
- Modify: `backend/crates/sober-agent/src/dispatch.rs`

- [ ] **Step 1: Instrument turn.rs functions**

Add `use tracing::instrument;` at the top of `turn.rs`, then:

```rust
// Line ~86:
#[instrument(skip(params), fields(
    conversation.id = %params.conversation_id,
    user.id = %params.user_id,
))]
pub async fn run_turn(...)

// Line ~528:
#[instrument(level = "debug", skip(params))]
async fn build_context(...)

// Line ~673:
#[instrument(level = "debug", skip(params, messages))]
async fn load_attachment_data(...)

// Line ~750 (if this is a separate fn from agent.rs):
#[instrument(level = "debug", skip(repos, llm_config, mek))]
async fn try_resolve_dynamic_engine(...)

// Line ~793:
#[instrument(level = "debug", skip(params, assistant_text))]
async fn auto_generate_title(...)
```

- [ ] **Step 2: Instrument dispatch.rs functions**

Add `use tracing::instrument;` at the top of `dispatch.rs`, then:

```rust
// Line ~99:
#[instrument(skip(ctx, req), fields(
    conversation.id = %req.conversation_id,
    user.id = %req.user_id,
    tool_count = req.tool_calls.len(),
))]
pub async fn execute_tool_calls(...)

// Line ~347:
#[instrument(skip(ctx, tool_registry, tool_input, event_tx), fields(tool.name = %tool_name))]
async fn execute_single_tool(...)

// Line ~443:
#[instrument(skip(ctx, tool, tool_input, event_tx))]
async fn handle_confirmation(...)

// Line ~532:
#[instrument(level = "debug", skip(event_tx, broadcast_tx, output, error, input), fields(tool.name = %tool_name))]
async fn send_execution_update(...)
```

- [ ] **Step 3: Add `.instrument(span)` to tool execution spawn**

Find the `tokio::spawn` in `dispatch.rs` (~line 363) used for panic catching around tool execution. Wrap the spawned future:

```rust
let tool_span = tracing::info_span!("tool.execute", tool.name = %tool_name);
let handle = tokio::spawn(async move {
    // ... existing tool execution code
}.instrument(tool_span));
```

Add `use tracing::Instrument;` to imports if not present.

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-agent -q`
Expected: Compiles without errors.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-agent/src/turn.rs \
       backend/crates/sober-agent/src/dispatch.rs
git commit -m "feat(agent): instrument agentic loop (run_turn, dispatch, tool execution)"
```

---

### Task 12: Instrument `sober-agent` — Tool implementations (part 1)

Add `#[instrument]` to `execute_inner` on shell, memory, artifacts, and scheduler tools.

**Files:**
- Modify: `backend/crates/sober-agent/src/tools/shell.rs`
- Modify: `backend/crates/sober-agent/src/tools/memory.rs`
- Modify: `backend/crates/sober-agent/src/tools/artifacts.rs`
- Modify: `backend/crates/sober-agent/src/tools/scheduler.rs`

- [ ] **Step 1: Instrument ShellTool**

Add `use tracing::instrument;` at the top of `shell.rs`, then:

```rust
// Line ~89:
#[instrument(skip(self, input), fields(tool.name = "shell"))]
async fn execute_inner(...)
```

- [ ] **Step 2: Instrument memory tools (RecallTool, RememberTool)**

Add `use tracing::instrument;` at the top of `memory.rs`, then:

```rust
// RecallTool::execute_inner, line ~116:
#[instrument(skip(self, input), fields(tool.name = "recall"))]
async fn execute_inner(...)

// RememberTool::execute_inner, line ~382:
#[instrument(skip(self, input), fields(tool.name = "remember"))]
async fn execute_inner(...)
```

- [ ] **Step 3: Instrument artifact tools**

Add `use tracing::instrument;` at the top of `artifacts.rs`, then:

```rust
// ArtifactsTool::execute_inner, line ~104:
#[instrument(skip(self, input), fields(tool.name = "artifacts"))]
async fn execute_inner(...)

// ArtifactCreateTool::execute_inner, line ~242:
#[instrument(skip(self, input), fields(tool.name = "artifact_create"))]
async fn execute_inner(...)

// ArtifactGetTool::execute_inner, line ~342:
#[instrument(skip(self, input), fields(tool.name = "artifact_get"))]
async fn execute_inner(...)

// ArtifactUpdateTool::execute_inner, line ~447:
#[instrument(skip(self, input), fields(tool.name = "artifact_update"))]
async fn execute_inner(...)
```

- [ ] **Step 4: Instrument SchedulerTool**

Add `use tracing::instrument;` at the top of `scheduler.rs`, then:

```rust
// Line ~411:
#[instrument(skip(self, input), fields(tool.name = "scheduler"))]
async fn execute_inner(...)
```

- [ ] **Step 5: Build and verify**

Run: `cd backend && cargo build -p sober-agent -q`
Expected: Compiles without errors.

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-agent/src/tools/shell.rs \
       backend/crates/sober-agent/src/tools/memory.rs \
       backend/crates/sober-agent/src/tools/artifacts.rs \
       backend/crates/sober-agent/src/tools/scheduler.rs
git commit -m "feat(agent): instrument shell, memory, artifacts, scheduler tool executors"
```

---

### Task 13: Instrument `sober-agent` — Tool implementations (part 2)

Add `#[instrument]` to `execute_inner` on secrets, snapshots, fetch_url, web_search, generate_plugin, and all propose_* tools.

**Files:**
- Modify: `backend/crates/sober-agent/src/tools/secrets.rs`
- Modify: `backend/crates/sober-agent/src/tools/snapshots.rs`
- Modify: `backend/crates/sober-agent/src/tools/fetch_url.rs`
- Modify: `backend/crates/sober-agent/src/tools/web_search.rs`
- Modify: `backend/crates/sober-agent/src/tools/generate_plugin.rs`
- Modify: `backend/crates/sober-agent/src/tools/propose_tool.rs`
- Modify: `backend/crates/sober-agent/src/tools/propose_skill.rs`
- Modify: `backend/crates/sober-agent/src/tools/propose_instruction.rs`
- Modify: `backend/crates/sober-agent/src/tools/propose_automation.rs`

- [ ] **Step 1: Instrument secrets tools**

Add `use tracing::instrument;` at the top of `secrets.rs`, then add `#[instrument(skip(self, input), fields(tool.name = "<name>"))]` before each `execute_inner`. There are 4 tool structs in secrets.rs — each has its own `execute_inner` (lines ~133, ~276, ~373, ~469). Use tool names from their `metadata()` implementations.

- [ ] **Step 2: Instrument snapshot tools**

Add `use tracing::instrument;` at the top of `snapshots.rs`, then add to each `execute_inner` (lines ~71, ~188, ~268):

```rust
#[instrument(skip(self, input), fields(tool.name = "create_snapshot"))]
// or "list_snapshots", "restore_snapshot" for the other two
```

- [ ] **Step 3: Instrument fetch_url, web_search, generate_plugin**

Add `use tracing::instrument;` to each file, then:

```rust
// fetch_url.rs, line ~218:
#[instrument(skip(self, input), fields(tool.name = "fetch_url"))]
async fn execute_inner(...)

// web_search.rs, line ~104:
#[instrument(skip(self, input), fields(tool.name = "web_search"))]
async fn execute_inner(...)

// generate_plugin.rs, line ~136:
#[instrument(skip(self, input), fields(tool.name = "generate_plugin"))]
async fn execute_inner(...)
```

- [ ] **Step 4: Instrument propose_* tools**

Add `use tracing::instrument;` to each file, then:

```rust
// propose_tool.rs, line ~92:
#[instrument(skip(self, input), fields(tool.name = "propose_tool"))]
async fn execute_inner(...)

// propose_skill.rs, line ~88:
#[instrument(skip(self, input), fields(tool.name = "propose_skill"))]
async fn execute_inner(...)

// propose_instruction.rs, line ~90:
#[instrument(skip(self, input), fields(tool.name = "propose_instruction"))]
async fn execute_inner(...)

// propose_automation.rs, line ~95:
#[instrument(skip(self, input), fields(tool.name = "propose_automation"))]
async fn execute_inner(...)
```

- [ ] **Step 5: Build and verify**

Run: `cd backend && cargo build -p sober-agent -q`
Expected: Compiles without errors.

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-agent/src/tools/secrets.rs \
       backend/crates/sober-agent/src/tools/snapshots.rs \
       backend/crates/sober-agent/src/tools/fetch_url.rs \
       backend/crates/sober-agent/src/tools/web_search.rs \
       backend/crates/sober-agent/src/tools/generate_plugin.rs \
       backend/crates/sober-agent/src/tools/propose_tool.rs \
       backend/crates/sober-agent/src/tools/propose_skill.rs \
       backend/crates/sober-agent/src/tools/propose_instruction.rs \
       backend/crates/sober-agent/src/tools/propose_automation.rs
git commit -m "feat(agent): instrument secrets, snapshots, fetch, search, plugin gen, propose tools"
```

---

### Task 14: Instrument `sober-agent` — Evolution and ingestion

Add `#[instrument]` to evolution executor and ingestion pipeline.

**Files:**
- Modify: `backend/crates/sober-agent/src/evolution/executor.rs`
- Modify: `backend/crates/sober-agent/src/ingestion.rs`

- [ ] **Step 1: Instrument evolution executor**

Add `use tracing::instrument;` at the top of `executor.rs`, then:

```rust
// Line ~67:
#[instrument(skip(repos, mind, ctx), fields(
    evolution.id = %event.id,
    evolution.type_ = %event.evolution_type,
    evolution.title = %event.title,
))]
pub async fn execute_evolution(...)

// Line ~159:
#[instrument(skip(ctx), fields(evolution.id = %event.id))]
async fn execute_plugin(...)

// Line ~247:
#[instrument(skip(ctx), fields(evolution.id = %event.id))]
async fn execute_skill(...)

// Line ~358:
#[instrument(skip(mind), fields(evolution.id = %event.id))]
async fn execute_instruction(...)

// Line ~381:
#[instrument(skip(scheduler_client), fields(evolution.id = %event.id))]
async fn execute_automation(...)
```

- [ ] **Step 2: Instrument ingestion**

Add `use tracing::instrument;` and `use tracing::Instrument;` at the top of `ingestion.rs`, then:

```rust
// Line ~19:
#[instrument(skip(llm, memory, extractions), fields(extraction_count = extractions.len()))]
pub fn spawn_extraction_ingestion(...)
```

Also wrap the `tokio::spawn` call (~line 30) with a span:

```rust
let ingest_span = tracing::info_span!(
    "ingestion.embed_and_store",
    user.id = %user_id,
    conversation.id = %conversation_id,
);
tokio::spawn(async move {
    // ... existing code
}.instrument(ingest_span));
```

- [ ] **Step 3: Build and verify**

Run: `cd backend && cargo build -p sober-agent -q`
Expected: Compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-agent/src/evolution/executor.rs \
       backend/crates/sober-agent/src/ingestion.rs
git commit -m "feat(agent): instrument evolution executor and memory ingestion pipeline"
```

---

### Task 15: Normalize `sober-agent` gRPC span fields

Normalize field names in existing manual spans to match the system convention, and add `.instrument(span)` to spawned tasks in gRPC handlers.

**Files:**
- Modify: `backend/crates/sober-agent/src/grpc/agent.rs`

- [ ] **Step 1: Verify field names in handle_message span**

Check the manual span at line ~35. It should already use `user.id`, `conversation.id`. If any field names differ from the convention (e.g., `user_id` instead of `user.id`), rename them. The current span (from our earlier read) already uses the correct dotted convention.

- [ ] **Step 2: Add `.instrument(span)` to spawned tasks**

The stream drainer task at line ~88 and the execute_task worker at line ~190 need spans. Add `use tracing::Instrument;` to imports, then:

For the drainer at line ~88:
```rust
let drainer_span = tracing::debug_span!("agent.drain_stream", conversation.id = %conversation_id);
tokio::spawn(async move {
    use futures::StreamExt;
    let mut stream = stream;
    while stream.next().await.is_some() {}
}.instrument(drainer_span));
```

For the execute_task worker at line ~190:
```rust
let task_span = tracing::info_span!(
    "agent.execute_task_worker",
    task.id = %task_id,
);
tokio::spawn(async move {
    // ... existing match on JobPayload
}.instrument(task_span));
```

- [ ] **Step 3: Build and verify**

Run: `cd backend && cargo build -p sober-agent -q`
Expected: Compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-agent/src/grpc/agent.rs
git commit -m "feat(agent): normalize gRPC span fields and instrument spawned tasks"
```

---

### Task 16: Wire ghost agent metrics

Emit the 4 declared-but-never-recorded agent metrics.

**Files:**
- Modify: `backend/crates/sober-agent/src/agent.rs`
- Modify: `backend/crates/sober-agent/src/dispatch.rs`

- [ ] **Step 1: Wire request metrics in Agent::handle_message**

In `agent.rs`, in the `handle_message` method (~line 384), add timing and counter:

```rust
pub async fn handle_message(...) -> Result<AgentResponseStream, AgentError> {
    let start = std::time::Instant::now();
    // ... existing code ...
    // Before returning Ok(stream):
    metrics::counter!("sober_agent_requests_total", "status" => "ok").increment(1);
    metrics::histogram!("sober_agent_request_duration_seconds").record(start.elapsed().as_secs_f64());
    // ... In error path:
    metrics::counter!("sober_agent_requests_total", "status" => "error").increment(1);
    metrics::histogram!("sober_agent_request_duration_seconds").record(start.elapsed().as_secs_f64());
```

- [ ] **Step 2: Wire tool call metrics in execute_tool_calls**

In `dispatch.rs`, in `execute_tool_calls` (~line 99), add timing per tool:

```rust
// Before each tool execution (in the loop or in execute_single_tool):
let tool_start = std::time::Instant::now();
// ... execute tool ...
metrics::counter!("sober_agent_tool_calls_total", "tool" => tool_name.to_owned(), "status" => status).increment(1);
metrics::histogram!("sober_agent_tool_call_duration_seconds", "tool" => tool_name.to_owned()).record(tool_start.elapsed().as_secs_f64());
```

The exact placement depends on where the tool execution result is available. Place the counter and histogram after the tool result is known, using `"ok"` or `"error"` for the status label.

- [ ] **Step 3: Build and verify**

Run: `cd backend && cargo build -p sober-agent -q`
Expected: Compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-agent/src/agent.rs \
       backend/crates/sober-agent/src/dispatch.rs
git commit -m "feat(agent): wire sober_agent_requests_total and tool call metrics"
```

---

### Task 17: Instrument `sober-scheduler` — Tick engine

Add `#[instrument]` to tick engine methods and wrap conditionally.

**Files:**
- Modify: `backend/crates/sober-scheduler/src/engine.rs`

- [ ] **Step 1: Instrument engine methods**

Add `use tracing::instrument;` at the top of `engine.rs`, then:

```rust
// Line ~106:
#[instrument(skip(self))]
pub async fn run(...)

// Line ~92:
#[instrument(skip(self))]
pub fn pause(...)

// Line ~99:
#[instrument(skip(self))]
pub fn resume(...)
```

- [ ] **Step 2: Add conditional span to tick()**

The `tick()` method (~line 133) should only create a meaningful span when there are due jobs. Add `#[instrument]` at debug level and a conditional info span inside:

```rust
// Line ~133:
#[instrument(level = "debug", skip(self))]
async fn tick(&self) {
    // ... existing code to query due jobs ...
    // After determining due_jobs:
    if !due_jobs.is_empty() {
        let _span = tracing::info_span!("scheduler.tick_execute", due_job_count = due_jobs.len()).entered();
        // ... existing job execution loop ...
    }
```

- [ ] **Step 3: Instrument route_job, execute_via_agent, wake_agent, force_run_job**

```rust
// Line ~307:
#[instrument(skip(agent_client, executor_registry, job), fields(job.id = %job.id, job.name = %job.name))]
async fn route_job(...)

// Line ~388:
#[instrument(skip(agent_client, job), fields(job.id = %job.id))]
async fn execute_via_agent(...)

// Line ~362:
#[instrument(level = "debug", skip(agent_client, job), fields(job.id = %job.id))]
async fn wake_agent(...)

// Line ~447:
#[instrument(skip(job_repo, run_repo, agent_client, executor_registry), fields(job.id = %job_id))]
pub async fn force_run_job(...)
```

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-scheduler -q`
Expected: Compiles without errors.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-scheduler/src/engine.rs
git commit -m "feat(scheduler): instrument tick engine with conditional spans and job routing"
```

---

### Task 18: Instrument `sober-scheduler` — Job executors and gRPC

Add `#[instrument]` to all executor `execute()` methods and gRPC handlers.

**Files:**
- Modify: `backend/crates/sober-scheduler/src/executors/artifact.rs`
- Modify: `backend/crates/sober-scheduler/src/executors/memory_pruning.rs`
- Modify: `backend/crates/sober-scheduler/src/executors/session_cleanup.rs`
- Modify: `backend/crates/sober-scheduler/src/executors/blob_gc.rs`
- Modify: `backend/crates/sober-scheduler/src/executors/attachment_cleanup.rs`
- Modify: `backend/crates/sober-scheduler/src/executors/plugin_cleanup.rs`
- Modify: `backend/crates/sober-scheduler/src/grpc.rs`

- [ ] **Step 1: Instrument all executor execute() methods**

For each executor file, add `use tracing::instrument;` and the attribute:

```rust
// artifact.rs, line ~43:
#[instrument(skip(self, job), fields(job.id = %job.id, job.name = %job.name))]
async fn execute(...)

// memory_pruning.rs, line ~32:
#[instrument(skip(self, job), fields(job.id = %job.id, job.name = %job.name))]
async fn execute(...)

// session_cleanup.rs, line ~23:
#[instrument(skip(self, _job))]
async fn execute(...)

// blob_gc.rs, line ~50:
#[instrument(skip(self, _job))]
async fn execute(...)

// attachment_cleanup.rs, line ~38:
#[instrument(skip(self, _job))]
async fn execute(...)

// plugin_cleanup.rs, line ~32:
#[instrument(skip(self, _job))]
async fn execute(...)
```

- [ ] **Step 2: Instrument gRPC handlers**

Add `use tracing::instrument;` at the top of `grpc.rs`, then add to each handler. Since these implement a trait, the attribute goes on the `async fn` directly:

```rust
// Line ~118:
#[instrument(skip(self, request))]
async fn create_job(...)

// Line ~170:
#[instrument(skip(self, request))]
async fn cancel_job(...)

// Line ~182:
#[instrument(level = "debug", skip(self, request))]
async fn list_jobs(...)

// Line ~221:
#[instrument(level = "debug", skip(self, request))]
async fn get_job(...)

// Line ~234:
#[instrument(level = "debug", skip(self, request))]
async fn list_job_runs(...)

// Line ~253:
#[instrument(skip(self, _request))]
async fn pause_scheduler(...)

// Line ~261:
#[instrument(skip(self, _request))]
async fn resume_scheduler(...)

// Line ~269:
#[instrument(skip(self, request))]
async fn force_run(...)

// Line ~300:
#[instrument(skip(self, request))]
async fn pause_job(...)

// Line ~322:
#[instrument(skip(self, request))]
async fn resume_job(...)

// Line ~362:
#[instrument(level = "debug", skip(self, _request))]
async fn health(...)
```

- [ ] **Step 3: Add `.instrument(span)` to force_run spawn**

In `grpc.rs` at line ~281, wrap the `tokio::spawn` in the `force_run` handler:

```rust
let force_span = tracing::info_span!("scheduler.force_run_worker", job.id = %job_id);
tokio::spawn(async move {
    // ... existing force_run_job call
}.instrument(force_span));
```

Add `use tracing::Instrument;` to imports.

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-scheduler -q`
Expected: Compiles without errors.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-scheduler/src/executors/ \
       backend/crates/sober-scheduler/src/grpc.rs
git commit -m "feat(scheduler): instrument job executors and gRPC handlers"
```

---

### Task 19: Instrument `sober-llm`

Add `#[instrument]` to LLM engine methods and SSE parsing.

**Files:**
- Modify: `backend/crates/sober-llm/src/client.rs`
- Modify: `backend/crates/sober-llm/src/acp.rs`
- Modify: `backend/crates/sober-llm/src/streaming.rs`

- [ ] **Step 1: Instrument OpenAiCompatibleEngine**

Add `use tracing::instrument;` at the top of `client.rs`, then:

```rust
// Line ~200:
#[instrument(skip(self, req), fields(model = %self.model, provider = %self.base_url))]
async fn complete(...)

// Line ~242:
#[instrument(skip(self, req), fields(model = %self.model, provider = %self.base_url))]
async fn stream(...)

// Line ~277:
#[instrument(skip(self, texts), fields(model = %self.embedding_model))]
async fn embed(...)
```

- [ ] **Step 2: Instrument AcpEngine**

Add `use tracing::instrument;` at the top of `acp.rs`, then:

```rust
// Line ~254:
#[instrument(skip(self, req), fields(model = %self.config.model))]
async fn complete(...)

// Line ~320:
#[instrument(skip(self, req), fields(model = %self.config.model))]
async fn stream(...)

// Line ~353:
#[instrument(skip(self, _texts), fields(model = %self.config.model))]
async fn embed(...)
```

- [ ] **Step 3: Instrument SSE parsing**

Add `use tracing::instrument;` at the top of `streaming.rs`, then:

```rust
// Line ~23:
#[instrument(level = "debug", skip(response))]
pub fn parse_sse_stream(...)
```

- [ ] **Step 4: Build and verify**

Run: `cd backend && cargo build -p sober-llm -q`
Expected: Compiles without errors.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-llm/src/client.rs \
       backend/crates/sober-llm/src/acp.rs \
       backend/crates/sober-llm/src/streaming.rs
git commit -m "feat(llm): instrument LLM engine methods and SSE parsing"
```

---

### Task 20: Metrics cleanup — Remove ghost metrics, document undocumented metrics

**Files:**
- Modify: `backend/crates/sober-core/metrics.toml`
- Modify: `backend/crates/sober-plugin/metrics.toml`
- Modify: `backend/crates/sober-api/metrics.toml`
- Modify: `backend/crates/sober-scheduler/metrics.toml`
- Modify: `backend/crates/sober-db/metrics.toml`
- Modify: `backend/crates/sober-workspace/metrics.toml`
- Modify: `backend/crates/sober-agent/metrics.toml`

- [ ] **Step 1: Remove ghost metrics from sober-core**

Remove all 4 process metrics from `backend/crates/sober-core/metrics.toml`:
- `sober_process_cpu_seconds_total`
- `sober_process_resident_memory_bytes`
- `sober_process_open_fds`
- `sober_process_uptime_seconds`

- [ ] **Step 2: Remove ghost metrics from sober-plugin**

Remove 3 metrics from `backend/crates/sober-plugin/metrics.toml`:
- `sober_plugin_installed`
- `sober_plugin_audit_runs_total`
- `sober_plugin_sandbox_violations_total`

- [ ] **Step 3: Add undocumented metrics to sober-api**

Add to `backend/crates/sober-api/metrics.toml`:

```toml
[[metrics]]
name = "sober_attachment_uploads_total"
type = "counter"
description = "Total attachment uploads"
labels = ["status"]

[[metrics]]
name = "sober_attachment_upload_bytes"
type = "counter"
description = "Total bytes uploaded via attachments"

[[metrics]]
name = "sober_attachment_upload_duration_seconds"
type = "histogram"
description = "Attachment upload duration"
```

- [ ] **Step 4: Add undocumented metrics to sober-scheduler**

Add to `backend/crates/sober-scheduler/metrics.toml`:

```toml
[[metrics]]
name = "sober_blob_gc_runs_total"
type = "counter"
description = "Total blob garbage collection runs"
labels = ["status"]

[[metrics]]
name = "sober_blob_gc_deleted_total"
type = "counter"
description = "Total blobs deleted by GC"

[[metrics]]
name = "sober_blob_gc_bytes_freed_total"
type = "counter"
description = "Total bytes freed by blob GC"

[[metrics]]
name = "sober_attachment_cleanup_deleted_total"
type = "counter"
description = "Total expired attachments cleaned up"
```

- [ ] **Step 5: Add undocumented metrics to sober-db, sober-workspace, sober-agent**

Add to `backend/crates/sober-db/metrics.toml`:

```toml
[[metrics]]
name = "sober_message_content_blocks_total"
type = "counter"
description = "Total message content blocks stored"
```

Add to `backend/crates/sober-workspace/metrics.toml`:

```toml
[[metrics]]
name = "sober_attachment_image_processing_seconds"
type = "histogram"
description = "Duration of attachment image processing (resize, thumbnail)"
```

Add to `backend/crates/sober-agent/metrics.toml`:

```toml
[[metrics]]
name = "sober_llm_vision_blocks_resolved_total"
type = "counter"
description = "Total vision content blocks resolved from attachments"
labels = ["status"]
```

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-core/metrics.toml \
       backend/crates/sober-plugin/metrics.toml \
       backend/crates/sober-api/metrics.toml \
       backend/crates/sober-scheduler/metrics.toml \
       backend/crates/sober-db/metrics.toml \
       backend/crates/sober-workspace/metrics.toml \
       backend/crates/sober-agent/metrics.toml
git commit -m "chore(metrics): remove 7 ghost metrics, document 9 undocumented metrics"
```

---

### Task 21: Add `sqlx::query=warn` to default filter strings

Enable sqlx query tracing at warn level in all binaries.

**Files:**
- Modify: `backend/crates/sober-api/src/main.rs:31`
- Modify: `backend/crates/sober-agent/src/main.rs:56`
- Modify: `backend/crates/sober-scheduler/src/main.rs:44`
- Modify: `backend/crates/sober-web/src/main.rs:51`

- [ ] **Step 1: Update all filter strings**

In each binary's `main.rs`, find the `init_telemetry` call and append `sqlx::query=warn`:

```rust
// sober-api main.rs, line 31:
sober_core::init_telemetry(config.environment, "sober_api=debug,tower_http=debug,sqlx::query=warn,info");

// sober-agent main.rs, line 56:
sober_core::init_telemetry(config.environment, "sober_agent=info,sober_mind=info,sober_memory=info,sqlx::query=warn,info");

// sober-scheduler main.rs, line 44:
sober_core::init_telemetry(config.environment, "sober_scheduler=info,sqlx::query=warn,info");

// sober-web main.rs, line 51:
sober_core::init_telemetry(environment, "sober_web=info,sqlx::query=warn,info");
```

- [ ] **Step 2: Build all binaries**

Run: `cd backend && cargo build --workspace -q`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-api/src/main.rs \
       backend/crates/sober-agent/src/main.rs \
       backend/crates/sober-scheduler/src/main.rs \
       backend/crates/sober-web/src/main.rs
git commit -m "feat(telemetry): enable sqlx::query=warn in all binary filter strings"
```

---

### Task 22: Full workspace build, clippy, test, and format check

Verify everything compiles together, passes lints, and existing tests pass.

**Files:** (none — verification only)

- [ ] **Step 1: Workspace build**

Run: `cd backend && cargo build --workspace -q`
Expected: Clean build.

- [ ] **Step 2: Clippy**

Run: `cd backend && cargo clippy --workspace -q -- -D warnings`
Expected: No warnings.

- [ ] **Step 3: Run tests**

Run: `cd backend && cargo test --workspace -q`
Expected: All tests pass.

- [ ] **Step 4: Format check**

Run: `cd backend && cargo fmt --check -q`
Expected: No formatting issues. If there are, run `cargo fmt` and commit:

```bash
cargo fmt
git add -u
git commit -m "style: format after observability instrumentation"
```
