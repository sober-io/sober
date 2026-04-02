# #051 API Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add structured request tracing and service-layer instrumentation to `sober-api` and `sober-core` so every request produces a rich trace tree with user/domain context.

**Architecture:** A custom `TraceLayer` creates HTTP-level spans with empty fields for user and request context. A `RequestContextLayer` middleware fills those fields from auth state and headers. All service methods get `#[instrument]` for domain-specific spans. `AppError` records error details on the current span.

**Tech Stack:** `tracing` (instrument macro), `tower-http` (TraceLayer customization), `tower` (Layer/Service traits for middleware)

---

### Task 1: Customize `TraceLayer` in `sober-api`

Replace the default `TraceLayer::new_for_http()` with a configured version that creates spans with semantic field names and classifies responses by status code.

**Files:**
- Modify: `backend/crates/sober-api/src/main.rs:58-63`

- [ ] **Step 1: Update imports in main.rs**

Add the necessary imports for the customized TraceLayer:

```rust
// Add these to the existing imports:
use std::time::Duration as StdDuration;

use axum::extract::MatchedPath;
use axum_core::body::Body;
use http::Response;
use tower_http::trace::TraceLayer;
use tracing::{info, info_span, Span};
```

Remove the existing `use tower_http::trace::TraceLayer;` import (line 23) since it will be covered by the new explicit import.

- [ ] **Step 2: Replace TraceLayer with customized version**

Replace `.layer(TraceLayer::new_for_http())` (line 61) with:

```rust
.layer(
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
        .on_response(|response: &Response<Body>, latency: StdDuration, span: &Span| {
            let status = response.status().as_u16();
            span.record("http.status_code", status);

            if status >= 500 {
                tracing::error!(latency_ms = latency.as_millis() as u64, "request failed");
            } else if status >= 400 {
                tracing::warn!(latency_ms = latency.as_millis() as u64, "client error");
            } else {
                tracing::info!(latency_ms = latency.as_millis() as u64, "request completed");
            }
        })
        .on_failure(
            |error: tower_http::classify::ServerErrorsFailureClass,
             latency: StdDuration,
             _span: &Span| {
                tracing::error!(
                    %error,
                    latency_ms = latency.as_millis() as u64,
                    "request error"
                );
            },
        ),
)
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
- Modify: `backend/crates/sober-api/src/main.rs`

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

The `RequestContextLayer` needs to see `AuthUser` in extensions, which is set by
`AuthLayer` inside `build_router`. Add it inside `build_router` in
`backend/crates/sober-api/src/routes/mod.rs`, between the auth layer and the
route merges:

```rust
use crate::middleware::request_context::RequestContextLayer;

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

`RequestContextLayer` is layered before `auth_layer` in `.layer()` order, which
means it runs **after** auth (layers execute inside-out). This ensures `AuthUser`
is present in extensions when `RequestContextService` reads it.

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

Record error details on the current span when an `AppError` is converted to an
HTTP response, so errors appear in traces automatically.

**Files:**
- Modify: `backend/crates/sober-core/src/error.rs:59-78`

- [ ] **Step 1: Update the `IntoResponse` implementation**

Add span recording before the response is constructed. The span fields
`otel.status_code`, `error.type_`, and `error.message` are declared as `Empty`
by the customized `TraceLayer` in Task 1.

Replace the `IntoResponse` impl:

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

        // Record error details on the current tracing span so they appear in
        // traces without each handler needing explicit error logging.
        let span = tracing::Span::current();
        span.record("otel.status_code", "ERROR");
        span.record("error.type_", error_type);
        span.record("error.message", &tracing::field::display(&self));

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

- [ ] **Step 2: Build and run existing tests**

Run: `cd backend && cargo build -p sober-core -q && cargo test -p sober-core -q`
Expected: All existing error tests still pass. The span recording is a no-op when
there is no active span (test environment), so existing tests are unaffected.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-core/src/error.rs
git commit -m "feat(core): record error details on current tracing span in AppError::into_response"
```

---

### Task 4: Instrument `ConversationService`

Add `#[instrument]` to all public methods on `ConversationService`.

**Files:**
- Modify: `backend/crates/sober-api/src/services/conversation.rs`

- [ ] **Step 1: Add tracing import**

Add at the top of the file:

```rust
use tracing::instrument;
```

- [ ] **Step 2: Add `#[instrument]` to all public methods**

Apply these attributes to each method in the `impl ConversationService` block:

```rust
#[instrument(level = "debug", skip(self))]
pub async fn list(...) -> ... {

#[instrument(skip(self))]
pub async fn create(...) -> ... {

#[instrument(level = "debug", skip(self), fields(conversation.id = %conversation_id))]
pub async fn get(...) -> ... {

#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn update(...) -> ... {

#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn delete(...) -> ... {

#[instrument(level = "debug", skip(self), fields(conversation.id = %conversation_id))]
pub async fn get_settings(...) -> ... {

#[instrument(skip(self, input), fields(conversation.id = %conversation_id))]
pub async fn update_settings(...) -> ... {

#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn mark_read(...) -> ... {

#[instrument(level = "debug", skip(self))]
pub async fn get_inbox(...) -> ... {

#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn convert_to_group(...) -> ... {

#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn clear_messages(...) -> ... {

#[instrument(level = "debug", skip(self), fields(conversation.id = %conversation_id))]
pub async fn list_jobs(...) -> ... {
```

- [ ] **Step 3: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-api/src/services/conversation.rs
git commit -m "feat(api): instrument ConversationService with tracing spans"
```

---

### Task 5: Instrument `MessageService`

**Files:**
- Modify: `backend/crates/sober-api/src/services/message.rs`

- [ ] **Step 1: Add tracing import and instrument methods**

Add `use tracing::instrument;` at the top, then apply:

```rust
#[instrument(level = "debug", skip(self), fields(conversation.id = %conversation_id))]
pub async fn list(&self, conversation_id: ConversationId, user_id: UserId, before: Option<MessageId>, limit: i64) -> ... {

#[instrument(skip(self), fields(message.id = %message_id))]
pub async fn delete(&self, message_id: MessageId, user_id: UserId) -> ... {
```

- [ ] **Step 2: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-api/src/services/message.rs
git commit -m "feat(api): instrument MessageService with tracing spans"
```

---

### Task 6: Instrument `AuthService`

**Files:**
- Modify: `backend/crates/sober-api/src/services/auth.rs`

- [ ] **Step 1: Add tracing import and instrument methods**

Add `use tracing::instrument;` at the top, then apply:

```rust
#[instrument(skip(self))]
pub async fn create_inbox_for_user(&self, user_id: UserId) -> ... {

#[instrument(level = "debug", skip(self))]
pub async fn get_user_with_roles(&self, user_id: UserId) -> ... {
```

- [ ] **Step 2: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-api/src/services/auth.rs
git commit -m "feat(api): instrument AuthService with tracing spans"
```

---

### Task 7: Instrument `UserService`

**Files:**
- Modify: `backend/crates/sober-api/src/services/user.rs`

- [ ] **Step 1: Add tracing import and instrument methods**

Add `use tracing::instrument;` at the top, then apply:

```rust
#[instrument(level = "debug", skip(self))]
pub async fn search(&self, query: &str, limit: i64) -> ... {
```

- [ ] **Step 2: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-api/src/services/user.rs
git commit -m "feat(api): instrument UserService with tracing spans"
```

---

### Task 8: Instrument `AttachmentService`

**Files:**
- Modify: `backend/crates/sober-api/src/services/attachment.rs`

- [ ] **Step 1: Add tracing import and instrument methods**

Add `use tracing::instrument;` at the top, then apply:

```rust
#[instrument(skip(self, data), fields(conversation.id = %conversation_id, attachment.filename = %filename))]
pub async fn upload(&self, conversation_id: ConversationId, user_id: UserId, filename: String, data: Vec<u8>) -> ... {
```

Note: `data` is skipped because it's a large binary payload.

- [ ] **Step 2: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-api/src/services/attachment.rs
git commit -m "feat(api): instrument AttachmentService with tracing spans"
```

---

### Task 9: Instrument `CollaboratorService`

**Files:**
- Modify: `backend/crates/sober-api/src/services/collaborator.rs`

- [ ] **Step 1: Add tracing import and instrument methods**

Add `use tracing::instrument;` at the top, then apply:

```rust
#[instrument(level = "debug", skip(self), fields(conversation.id = %conversation_id))]
pub async fn list(&self, conversation_id: ConversationId, user_id: UserId) -> ... {

#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn add(&self, conversation_id: ConversationId, caller_user_id: UserId, target_username: &str) -> ... {

#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn update_role(&self, conversation_id: ConversationId, caller_user_id: UserId, target_user_id: UserId, role: ConversationUserRole) -> ... {

#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn remove(&self, conversation_id: ConversationId, caller_user_id: UserId, target_user_id: UserId) -> ... {

#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn leave(&self, conversation_id: ConversationId, user_id: UserId) -> ... {
```

- [ ] **Step 2: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-api/src/services/collaborator.rs
git commit -m "feat(api): instrument CollaboratorService with tracing spans"
```

---

### Task 10: Instrument `EvolutionService`

**Files:**
- Modify: `backend/crates/sober-api/src/services/evolution.rs`

- [ ] **Step 1: Add tracing import and instrument methods**

Add `use tracing::instrument;` at the top, then apply:

```rust
#[instrument(level = "debug", skip(self))]
pub async fn list_events(&self, evolution_type: Option<EvolutionType>, status: Option<EvolutionStatus>) -> ... {

#[instrument(level = "debug", skip(self), fields(evolution.id = %id))]
pub async fn get_event(&self, id: EvolutionEventId) -> ... {

#[instrument(skip(self), fields(evolution.id = %id))]
pub async fn update_event(&self, id: EvolutionEventId, target_status: EvolutionStatus, admin_user_id: UserId) -> ... {

#[instrument(level = "debug", skip(self))]
pub async fn get_config(&self) -> ... {

#[instrument(skip(self, input))]
pub async fn update_config(&self, input: UpdateConfigInput) -> ... {

#[instrument(level = "debug", skip(self))]
pub async fn get_timeline(&self, limit: i64, evolution_type: Option<EvolutionType>, status: Option<EvolutionStatus>) -> ... {
```

- [ ] **Step 2: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-api/src/services/evolution.rs
git commit -m "feat(api): instrument EvolutionService with tracing spans"
```

---

### Task 11: Instrument `PluginService`

**Files:**
- Modify: `backend/crates/sober-api/src/services/plugin.rs`

- [ ] **Step 1: Add tracing import and instrument methods**

Add `use tracing::instrument;` at the top, then apply:

```rust
#[instrument(level = "debug", skip(self))]
pub async fn list(&self, kind: Option<String>, status: Option<String>, workspace_id: Option<String>) -> ... {

#[instrument(skip(self, config))]
pub async fn install(&self, name: String, kind: String, config: serde_json::Value, description: Option<String>, version: Option<String>) -> ... {

#[instrument(skip(self, mcp_servers))]
pub async fn import(&self, mcp_servers: serde_json::Value) -> ... {

#[instrument(skip(self))]
pub async fn reload(&self) -> ... {

#[instrument(level = "debug", skip(self), fields(plugin.id = %id))]
pub async fn get(&self, id: uuid::Uuid) -> ... {

#[instrument(skip(self, config), fields(plugin.id = %id))]
pub async fn update(&self, id: uuid::Uuid, enabled: Option<bool>, config: Option<serde_json::Value>, scope: Option<String>) -> ... {

#[instrument(skip(self), fields(plugin.id = %id))]
pub async fn uninstall(&self, id: uuid::Uuid) -> ... {

#[instrument(level = "debug", skip(self), fields(plugin.id = %id))]
pub async fn list_audit_logs(&self, id: uuid::Uuid, limit: i64) -> ... {

#[instrument(level = "debug", skip(self))]
pub async fn list_skills(&self, user_id: UserId, conversation_id: Option<String>) -> ... {

#[instrument(skip(self))]
pub async fn reload_skills(&self, user_id: UserId, conversation_id: Option<String>) -> ... {

#[instrument(level = "debug", skip(self))]
pub async fn list_tools(&self) -> ... {
```

- [ ] **Step 2: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-api/src/services/plugin.rs
git commit -m "feat(api): instrument PluginService with tracing spans"
```

---

### Task 12: Instrument `TagService`

**Files:**
- Modify: `backend/crates/sober-api/src/services/tag.rs`

- [ ] **Step 1: Add tracing import and instrument methods**

Add `use tracing::instrument;` at the top, then apply:

```rust
#[instrument(level = "debug", skip(self))]
pub async fn list_by_user(&self, user_id: UserId) -> ... {

#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn add_to_conversation(&self, conversation_id: ConversationId, user_id: UserId, name: String) -> ... {

#[instrument(skip(self), fields(conversation.id = %conversation_id, tag.id = %tag_id))]
pub async fn remove_from_conversation(&self, conversation_id: ConversationId, user_id: UserId, tag_id: TagId) -> ... {

#[instrument(skip(self), fields(message.id = %message_id))]
pub async fn add_to_message(&self, message_id: MessageId, user_id: UserId, name: String) -> ... {

#[instrument(skip(self), fields(message.id = %message_id, tag.id = %tag_id))]
pub async fn remove_from_message(&self, message_id: MessageId, user_id: UserId, tag_id: TagId) -> ... {
```

- [ ] **Step 2: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-api/src/services/tag.rs
git commit -m "feat(api): instrument TagService with tracing spans"
```

---

### Task 13: Instrument `WsDispatchService` and `verify_membership`

Extend the existing tracing in `WsDispatchService` with `#[instrument]` on all
methods (the manual span in `send_message` stays as-is for gRPC context propagation).
Also instrument the shared `verify_membership` function.

**Files:**
- Modify: `backend/crates/sober-api/src/services/ws_dispatch.rs`
- Modify: `backend/crates/sober-api/src/services/verify_membership.rs`

- [ ] **Step 1: Add `#[instrument]` to WsDispatchService methods**

Add `use tracing::instrument;` at the top of `ws_dispatch.rs`, then apply:

```rust
#[instrument(skip(self), fields(conversation.id = %conversation_id))]
pub async fn subscribe(&self, conversation_id: ConversationId, user_id: UserId) -> ... {

#[instrument(skip(self, content, error_tx), fields(conversation.id = %conversation_id))]
pub async fn send_message(&self, conversation_id: ConversationId, user_id: UserId, username: &str, content: Vec<ContentBlock>, error_tx: mpsc::Sender<ServerWsMessage>) -> ... {

#[instrument(skip(self))]
pub async fn confirm_response(&self, confirm_id: String, approved: bool) -> ... {

#[instrument(skip(self))]
pub async fn set_permission_mode(&self, mode: String) -> ... {
```

- [ ] **Step 2: Instrument `verify_membership`**

Add `use tracing::instrument;` at the top of `verify_membership.rs`, then apply:

```rust
#[instrument(level = "debug", skip(db), fields(conversation.id = %conversation_id))]
pub(crate) async fn verify_membership(
    db: &PgPool,
    conversation_id: ConversationId,
    user_id: UserId,
) -> Result<ConversationUser, AppError> {
```

- [ ] **Step 3: Build and verify**

Run: `cd backend && cargo build -p sober-api -q`
Expected: Compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-api/src/services/ws_dispatch.rs \
       backend/crates/sober-api/src/services/verify_membership.rs
git commit -m "feat(api): instrument WsDispatchService and verify_membership"
```

---

### Task 14: Instrument `sober-web` proxy handlers

Add `#[instrument]` to the reverse proxy and WebSocket proxy handlers.

**Files:**
- Modify: `backend/crates/sober-web/src/main.rs:132-171`

- [ ] **Step 1: Add tracing import and instrument proxy handlers**

Add `use tracing::instrument;` to the imports in `main.rs`, then apply:

```rust
#[instrument(skip_all, fields(upstream.path = %original_uri.path()))]
async fn reverse_proxy(
    State(state): State<ProxyState>,
    original_uri: axum::extract::OriginalUri,
    mut req: Request<Body>,
) -> Result<Response, StatusCode> {
```

```rust
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

### Task 15: Full workspace build and test

Verify everything compiles together and existing tests pass.

**Files:** (none — verification only)

- [ ] **Step 1: Workspace build**

Run: `cd backend && cargo build --workspace -q`
Expected: Clean build.

- [ ] **Step 2: Clippy**

Run: `cd backend && cargo clippy --workspace -q -- -D warnings`
Expected: No warnings.

- [ ] **Step 3: Run tests**

Run: `cd backend && cargo test --workspace -q`
Expected: All tests pass (including `sober-core` error tests).

- [ ] **Step 4: Format check**

Run: `cd backend && cargo fmt --check -q`
Expected: No formatting issues.

- [ ] **Step 5: Commit any fixes if needed**

If clippy or fmt required changes:

```bash
git add -u
git commit -m "fix(api): address clippy and formatting issues from observability changes"
```
