## Rust Patterns — Sõber Project

### Ownership & Borrowing

- **Borrow by default.** `&T` to read, `&mut T` to modify. Only take ownership (`T`) when storing or consuming.
- Prefer `&str` over `&String`, `&[T]` over `&Vec<T>` in function parameters.
- **Structs own their data** (`String`, not `&str`) unless performance-critical.
- Use `Cow<'_, str>` when you sometimes need to allocate and sometimes don't.
- Clone deliberately — not as a borrow-checker escape hatch.
- Don't annotate lifetimes unless the compiler asks. Use descriptive names (`'input`, `'conn`).
- `Arc<T>` for shared state across handlers/tasks. `RwLock` over `Mutex` when reads dominate.
- Don't hold locks across `.await` points — clone what you need, drop the guard, then await.

### Error Handling

- Central `AppError` enum in `sober-core/src/error.rs` with `thiserror`. Variants: `NotFound`, `Validation`, `Unauthorized`, `Forbidden`, `Conflict`, `Internal`.
- `IntoResponse` maps each variant to HTTP status + JSON envelope `{ "error": { "code": "...", "message": "..." } }`.
- Domain crates define their own error enums (e.g., `AgentError`) and implement `From<DomainError> for AppError`.
- All handlers return `Result<impl IntoResponse, AppError>` — enables `?` throughout.
- `thiserror` for library errors, `anyhow` only in binaries.
- No `.unwrap()` in library code. Use `.expect("reason")` only when state is provably impossible.

### API Response Envelope

- Success: `ApiResponse<T>` → `{ "data": T }` with HTTP 200. Constructor is `#[must_use]`.
- Error: `ApiErrorEnvelope` → `{ "error": { "code": "...", "message": "..." } }`.
- Both implement `IntoResponse`. Defined in `sober-core/src/types/api.rs`.

### Handler Pattern

- **Thin handlers** — extract HTTP input (via axum extractors), call service, wrap in `ApiResponse`. Zero business logic.
- State injected via `State(Arc<AppState>)`.
- Auth via `AuthUser` extractor (set by middleware). `RequireAdmin` wrapper for role-gated endpoints.
- Business logic lives in `services/` module; handlers never construct repos directly.

### Service Layer Pattern

- Service structs hold `PgPool` + needed clients (e.g., `AgentClient`, `ConnectionRegistry`).
- Constructed in `AppState::new()` and `::from_parts()`, wrapped in `Arc`.
- Methods return `Result<T, AppError>` with typed response DTOs (replace `serde_json::json!()`).
- Multi-table operations use `_tx` repo methods within a service-level transaction.
- WS broadcasts and other side effects happen **outside** the transaction.
- Repos are instantiated per method call: `PgFooRepo::new(self.db.clone())`.

### Transaction Composition (`_tx` pattern)

- Repos provide `_tx` method variants: `pub async fn method_tx(conn: &mut PgConnection, ...) -> Result<T, AppError>`.
- Services call `self.db.begin()`, compose `_tx` calls on `&mut *tx`, then `tx.commit()`.
- Read-only operations and single-table writes use pool-based repo methods directly.
- The `_tx` methods are associated functions on the repo struct (not trait methods), accepting `&mut PgConnection`.

### Repository Pattern

- Abstract repo traits in `sober-core/src/types/repo.rs` (no sqlx dependency).
- Uses **RPITIT** (`impl Future` in trait methods) — no `async_trait` macro.
- Concrete `Pg*Repo` implementations in `sober-db/src/repos/`.
- Private row types (`#[derive(sqlx::FromRow)]`) convert to domain types via `From<Row>`.
- Query pattern: `sqlx::query_as::<_, RowType>()` with `.bind(id.as_uuid())`.

### ID Newtypes

- `define_id!` macro in `sober-core/src/types/ids.rs` generates newtype wrappers around `Uuid`.
- Derives: `Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize`.
- Feature-gated: `#[cfg_attr(feature = "postgres", derive(sqlx::Type))]`.
- Methods: `new()`, `from_uuid()`, `as_uuid()`, `Display`, `Default`.

### Domain Types

- Derives: `Debug, Clone, Serialize, Deserialize`. Own all data (`String`, not `&str`).
- Enums: add `Copy, PartialEq, Eq, Hash` + `#[serde(rename_all = "lowercase")]` + `#[sqlx(type_name = "...", rename_all = "lowercase")]`.
- All structs use `DateTime<Utc>` for timestamps.

### gRPC Patterns

- Proto files in `backend/proto/`. Package naming: `sober.<service>.v1`.
- Typed `oneof` envelopes for event streams (`AgentEvent`, `ConversationUpdate`).
- Services generic over repo types: `AgentGrpcService<Msg, Conv, Mcp>`.
- Streaming via `ReceiverStream` wrapping tokio channels.
- UDS connection via `tonic` + `hyper_util::TokioIo` + `tower::service_fn`.

### Config & State

- `AppConfig` in `sober-core/src/config.rs` — nested structs per subsystem, loaded from env vars.
- `AppState` holds `PgPool`, `AgentClient`, `Arc<AuthService<...>>`, `AppConfig`, `ConnectionRegistry`, and `Arc<*Service>` for each domain service.
- `AppState::new()` for production (connects to DB + gRPC), `::from_parts()` for tests. Both construct all services.
- Always wrapped in `Arc` for sharing across handlers.

### Module Organization

- `lib.rs` declares submodules and re-exports key types for ergonomic imports.
- `types/mod.rs` re-exports all submodules (`ids`, `domain`, `enums`, `repo`, `input`, `api`).
- Routes: separate module per group, each exports `pub fn routes() -> Router<Arc<AppState>>`.

### Testing

- Unit tests colocated in `#[cfg(test)]` modules.
- Integration tests use `#[sqlx::test(migrations = "../../migrations")]` for fresh per-test DB.
- Test harness structs (e.g., `TestAuth`) + helper functions (`register_and_login()`, `body_json()`).
- Route testing via `tower::ServiceExt::oneshot()` — no server startup.
