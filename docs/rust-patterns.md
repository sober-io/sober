## Rust Best Practices

### Ownership, Borrowing & Lifetimes

These are the core concepts that make Rust unique. Internalize these rules — they inform every design decision.

**The Three Ownership Rules:**
1. Each value has exactly one owner.
2. There can only be one owner at a time. Assignment moves ownership (for non-Copy types).
3. When the owner goes out of scope, the value is dropped.

**Borrowing Rules:**
- You can have EITHER one `&mut T` (exclusive/mutable) OR any number of `&T` (shared/immutable) — never both simultaneously.
- References must always be valid (no dangling pointers).
- The borrow checker enforces these at compile time, preventing data races by construction.

**Practical Guidelines:**

- **Borrow by default.** If a function only reads data, take `&T`. If it needs to modify, take `&mut T`. Only take ownership (`T`) when the function needs to store or consume the value.
  ```rust
  // Good: borrows — caller keeps ownership
  fn process(data: &MyStruct) -> Result<Output, AppError> { ... }

  // Good: needs to store it, so takes ownership
  fn register(user: User) -> Result<UserId, AppError> { ... }
  ```

- **Parse, don't validate.** Use newtypes that enforce invariants at construction. Once you have a `ValidEmail`, you know it's valid — no need to re-check.
  ```rust
  pub struct Email(String);

  impl Email {
      pub fn parse(s: impl Into<String>) -> Result<Self, ValidationError> {
          let s = s.into();
          // validate format...
          Ok(Email(s))
      }

      pub fn as_str(&self) -> &str {
          &self.0
      }
  }
  ```

- **Prefer `&str` over `&String` in function parameters.** `&str` is strictly more general — it accepts both `&String` and string literals. Same principle: prefer `&[T]` over `&Vec<T>`.

- **Use `Cow<'_, str>` when you sometimes need to allocate and sometimes don't.** Cow (Clone on Write) avoids unnecessary allocations:
  ```rust
  use std::borrow::Cow;

  fn normalize_name(input: &str) -> Cow<'_, str> {
      if input.contains(' ') {
          Cow::Owned(input.trim().to_lowercase())
      } else {
          Cow::Borrowed(input) // no allocation needed
      }
  }
  ```

- **Use `.clone()` deliberately, not as a borrow-checker escape hatch.** If you're cloning to silence the compiler, restructure the code first. Legitimate uses: small Copy-like data, shared state setup, or when the API genuinely needs owned data.

- **Understand Copy vs Clone.** Copy types (integers, bools, `char`, tuples of Copy types) are implicitly duplicated on assignment — no ownership transfer. Clone is explicit and can be expensive. Derive `Copy` on small, stack-only structs when appropriate.

- **Lifetimes are usually inferred.** Don't annotate lifetimes unless the compiler asks. When you do annotate, use descriptive names (`'input`, `'conn`) not just `'a` — it helps clarify which borrow is which.

- **For structs: own your data by default.** Use `String` not `&str` in structs unless you have a clear performance reason to borrow. Borrowed structs require lifetime annotations and are harder to pass around.
  ```rust
  // Prefer this for most application-level structs
  struct User {
      id: UserId,
      email: Email,
      name: String,
  }

  // Only use borrowed fields when performance-critical
  struct LogEntry<'a> {
      level: Level,
      message: &'a str,  // hot path, avoid allocation
  }
  ```

- **Smart pointers for shared ownership:**
  - `Rc<T>` — single-threaded shared ownership (reference counted).
  - `Arc<T>` — thread-safe shared ownership (atomic reference counted). Use for shared state in axum via `State(Arc<AppState>)`.
  - `RwLock` over `Mutex` when reads vastly outnumber writes.
  - Always drop read locks before acquiring write locks to avoid deadlock.

### Error Handling

- **Use `thiserror` for domain/library errors, `anyhow` only in main/scripts.**
- Define a central `AppError` enum:
  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum AppError {
      #[error("Not found: {0}")]
      NotFound(String),
      #[error("Validation error: {0}")]
      Validation(String),
      #[error("Unauthorized")]
      Unauthorized,
      #[error("Forbidden")]
      Forbidden,
      #[error("Conflict: {0}")]
      Conflict(String),
      #[error(transparent)]
      Internal(#[from] anyhow::Error),
  }
  ```
- Implement `IntoResponse` for `AppError` to map variants to HTTP status codes:
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
          let body = Json(serde_json::json!({
              "error": { "code": error_type, "message": self.to_string() }
          }));
          (status, body).into_response()
      }
  }
  ```
- All route handlers return `Result<impl IntoResponse, AppError>`. This enables `?` throughout.
- Propagate errors with `?` — use `From` implementations to convert between error types automatically.
- **Never `.unwrap()` in production code** unless the state is provably impossible (and add a comment explaining why). Prefer `.expect("reason")` at minimum.

### Type-Driven Design

- **Newtypes for domain concepts:** `struct UserId(Uuid)`, `struct Amount(Decimal)`. Prevents mixing up IDs of different entities.
- **`#[must_use]`** on functions returning important values (Results, computed data).
- **Enums for state machines.** Rust enums are sum types — use them to make illegal states unrepresentable:
  ```rust
  enum OrderStatus {
      Draft,
      Submitted { submitted_at: DateTime<Utc> },
      Paid { paid_at: DateTime<Utc>, amount: Amount },
      Shipped { tracking: TrackingNumber },
  }
  ```
- **Use `Option<T>` over sentinel values.** Never use `-1`, `""`, or `null` equivalents.
- **Use `Result<T, E>` for all fallible operations.** Pattern match exhaustively.

### Framework & Crates

- **axum** — HTTP framework (tower-based, async-native, composable, macro-free routing)
- **tokio** — async runtime
- **serde** / **serde_json** — serialization
- **sqlx** — async, compile-time checked SQL (if using a DB)
- **tracing** + **tracing-subscriber** — structured logging (not `println!`)
- **thiserror** — derive `Error` for custom error types
- **anyhow** — quick error handling in main/scripts only
- **dotenvy** — `.env` loading
- **tower-http** — CORS, compression, request tracing middleware
- **validator** — request body validation with derive macros
- **cargo-audit** — scan dependencies for known vulnerabilities
- **cargo-nextest** — faster parallel test runner

### Architecture Principles

- **Thin handlers:** Route handlers only parse input (via extractors), call a service, and return a response. Zero business logic in handlers.
- **Service layer:** Business logic in plain async functions or structs, injected via axum's `State` extractor.
- **Extractors over middleware** where possible — axum extractors are composable and type-safe.
- **Builder pattern** for complex structs, `Default` for config.
- **Favor immutability.** Variables are immutable by default in Rust — keep them that way unless mutation is needed.
- **Use `impl Trait` in function signatures** for cleaner APIs: `fn process(input: impl AsRef<str>)` instead of generic bounds when there's only one.
- **Minimize `unsafe`.** If you must use it, isolate it in a small module with a safe public API and document the safety invariants.

### Async Patterns

- Use `tokio::spawn` for truly concurrent work; `tokio::join!` for running futures concurrently without spawning tasks.
- Be aware of **cancellation safety** — if a future is dropped mid-await, any side effects before the await point have already happened.
- **Don't hold locks across `.await` points** — this can cause deadlocks or block the runtime. Scope lock guards tightly:
  ```rust
  // Bad: lock held across await
  let data = state.cache.read().await;
  let result = fetch_remote(&data).await; // lock still held!

  // Good: clone what you need, release lock
  let key = {
      let data = state.cache.read().await;
      data.key.clone()
  }; // lock dropped here
  let result = fetch_remote(&key).await;
  ```
- **Prefer `RwLock` over `Mutex`** when reads vastly outnumber writes.
- Use `Arc<T>` for state shared across handlers/tasks.

### API Design

- RESTful JSON API under `/api/v1/`.
- Use axum's `Json<T>` extractor and response type.
- Consistent response envelope:
  ```json
  { "data": ... }
  { "error": { "code": "NOT_FOUND", "message": "..." } }
  ```
- Validate request bodies using **validator** derive macros or a custom `Validate` trait.
- Use `#[debug_handler]` from `axum-macros` during development for better compiler error messages.

### Testing

- Unit tests in `#[cfg(test)]` modules colocated with the code.
- Integration tests in `tests/` using a shared test harness. Use `tower::ServiceExt::oneshot` to test routes without starting a server:
  ```rust
  #[tokio::test]
  async fn test_health() {
      let app = build_app().await;
      let response = app
          .oneshot(Request::builder().uri("/api/v1/health").body(Body::empty()).unwrap())
          .await
          .unwrap();
      assert_eq!(response.status(), StatusCode::OK);
  }
  ```
- Use **cargo-nextest** for faster parallel test execution.
- Run `cargo clippy -- -W clippy::all -W clippy::pedantic` in CI.

### Dependency Management & Security

- Run `cargo audit` regularly (and in CI) to check for known vulnerabilities.
- Evaluate crates before adding: check maintenance status, download count, and `unsafe` usage.
- Enable integer overflow checks in release builds in `Cargo.toml`:
  ```toml
  [profile.release]
  overflow-checks = true
  ```
- Prefer `rustls` over `openssl` for TLS (pure Rust, easier to cross-compile).
- Use `aws-lc-rs` as the crypto backend — the community is migrating away from `ring`.

---
---
