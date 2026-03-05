# Claude Code Bootstrap — Rust + Svelte Full-Stack Web App

## Project Overview

Bootstrap a full-stack web application with a **Rust backend** and **Svelte frontend**. Set up the project structure, tooling, and foundational patterns before any feature work begins.

---

## Tech Stack

- **Backend:** Rust (latest stable)
- **Frontend:** Svelte 5 (with SvelteKit)
- **Build:** Cargo workspaces for Rust, Vite via SvelteKit for frontend
- **Runtime:** Node.js 24
- **Package manager:** pnpm for JS, cargo for Rust

---

## Project Structure

Create a monorepo with clear separation:

```
project-root/
├── backend/            # Rust workspace
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── config.rs       # Typed config from env
│       ├── error.rs        # Centralized error types
│       ├── routes/         # Route handlers (thin layer)
│       ├── services/       # Business logic
│       ├── models/         # Domain types & DB models
│       └── middleware/     # Auth, logging, etc.
├── frontend/           # SvelteKit app
│   ├── package.json
│   ├── svelte.config.js
│   ├── src/
│   │   ├── lib/            # Shared components, utils, stores
│   │   │   ├── components/ # Reusable UI components
│   │   │   ├── stores/     # Svelte stores (global state)
│   │   │   └── utils/      # Helper functions
│   │   ├── routes/         # SvelteKit file-based routing
│   │   └── app.html
│   └── static/
├── shared/             # Shared types/contracts (optional)
├── .env.example
├── README.md
└── justfile            # Task runner (or Makefile)
```

---

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

## Svelte 5 Frontend — Patterns & Approaches

### SvelteKit Setup

- Use **SvelteKit** with the **static adapter** or **node adapter** depending on deployment (start with static if the Rust backend serves the API).
- **TypeScript** everywhere — `<script lang="ts">`.
- **Svelte 5 runes only** — do NOT use legacy `$:` reactive declarations, `export let` for props, `createEventDispatcher`, or `<slot>`. These are all deprecated patterns.

### Runes — The Reactivity System

Svelte 5 replaces implicit reactivity with explicit **runes** — compiler-recognized symbols that make reactive intent clear. Runes work in both `.svelte` files and `.svelte.ts` / `.svelte.js` modules.

**`$state` — Reactive state:**
```svelte
<script lang="ts">
  let count = $state(0);
  let items = $state<string[]>([]);

  // Direct mutation works — no need for reassignment like in Svelte 4
  function addItem(item: string) {
    items.push(item); // this triggers updates!
  }
</script>
```

**`$derived` — Computed values (pure, no side effects):**
```svelte
<script lang="ts">
  let width = $state(10);
  let height = $state(20);

  // Simple expression — recalculates when width or height change
  const area = $derived(width * height);

  // Complex computation — use $derived.by for multi-line logic
  const summary = $derived.by(() => {
    if (area > 100) return 'large';
    if (area > 50) return 'medium';
    return 'small';
  });
</script>
```
Use `$derived` for **all** computed values. Never duplicate state that can be derived.

**`$effect` — Side effects (sparingly!):**
```svelte
<script lang="ts">
  let searchQuery = $state('');

  // Runs after DOM update when dependencies change
  $effect(() => {
    console.log('Query changed:', searchQuery);

    // Return a cleanup function (runs before re-execution or unmount)
    return () => {
      console.log('Cleaning up previous effect');
    };
  });
</script>
```

**Critical `$effect` rules:**
- **Prefer `$derived` over `$effect`** for pure computations. `$effect` is for I/O, DOM interactions, and imperative side effects only.
- **Keep effects focused** — one effect, one job. Don't combine data fetching and DOM manipulation.
- **Dependencies are tracked synchronously.** Values read after `await` or inside `setTimeout` are NOT tracked — access reactive values before any async boundary:
  ```ts
  // BAD: size is not tracked (read after await)
  $effect(() => {
    const res = await fetch('/api');
    console.log(size); // NOT a dependency!
  });

  // GOOD: read size synchronously, then await
  $effect(() => {
    const currentSize = size; // tracked
    fetch(`/api?size=${currentSize}`).then(/* ... */);
  });
  ```
- **Don't use `$effect` to synchronize state** — this is almost always a sign you should use `$derived` instead.
- `$effect` does NOT run during SSR. Use `onMount` for lifecycle work that also needs server awareness.

### Props — `$props()`

Replace `export let` with the `$props()` rune. Always type your props with an interface:

```svelte
<!-- Button.svelte -->
<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    variant?: 'primary' | 'secondary';
    disabled?: boolean;
    onclick?: (e: MouseEvent) => void;
    children?: Snippet;
  }

  let {
    variant = 'primary',
    disabled = false,
    onclick,
    children,
  }: Props = $props();
</script>

<button class={variant} {disabled} {onclick}>
  {#if children}
    {@render children()}
  {/if}
</button>
```

**Spread remaining props** with the rest pattern for wrapper components:
```svelte
<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements';

  interface Props extends HTMLButtonAttributes {
    variant?: 'primary' | 'secondary';
  }

  let { variant = 'primary', ...rest }: Props = $props();
</script>

<button class={variant} {...rest} />
```

**Bindable props** — use `$bindable` when the parent needs two-way binding:
```svelte
<!-- TextInput.svelte -->
<script lang="ts">
  let { value = $bindable('') }: { value: string } = $props();
</script>

<input bind:value />
```
Only mark props as `$bindable` when genuinely needed. Don't mutate state you don't own.

### Events — Callback Props

Svelte 5 replaces `createEventDispatcher` and `on:` directives with **callback props** and **native DOM event attributes**:

```svelte
<!-- Svelte 5: callback props (do this) -->
<script lang="ts">
  let { onSave }: { onSave?: (data: FormData) => void } = $props();
</script>
<button onclick={() => onSave?.(formData)}>Save</button>

<!-- DOM events use lowercase attributes -->
<input oninput={(e) => (query = e.currentTarget.value)} />
<form onsubmit|preventDefault={handleSubmit}>
```

This increases type safety — the compiler can verify that a component actually accepts a given callback.

### Snippets — Replacing Slots

Svelte 5 replaces `<slot>` with **snippets** and `{@render}`. Snippets are more powerful, more explicit, and composable:

```svelte
<!-- Card.svelte -->
<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    header: Snippet;
    children: Snippet;
    footer?: Snippet;
  }

  let { header, children, footer }: Props = $props();
</script>

<div class="card">
  <div class="card-header">{@render header()}</div>
  <div class="card-body">{@render children()}</div>
  {#if footer}
    <div class="card-footer">{@render footer()}</div>
  {/if}
</div>
```

Usage:
```svelte
<Card>
  {#snippet header()}
    <h2>Title</h2>
  {/snippet}

  <p>Body content — this is the implicit `children` snippet.</p>

  {#snippet footer()}
    <button>Action</button>
  {/snippet}
</Card>
```

**Snippets with parameters** (replaces `let:` directive):
```svelte
<!-- List.svelte -->
<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props<T> {
    items: T[];
    row: Snippet<[T, number]>;
    empty?: Snippet;
  }

  let { items, row, empty }: Props<any> = $props();
</script>

{#if items.length === 0 && empty}
  {@render empty()}
{:else}
  {#each items as item, index}
    {@render row(item, index)}
  {/each}
{/if}
```

### Shared / Global State with Runes

Runes work in `.svelte.ts` / `.svelte.js` files — this enables **universal reactivity** outside components:

```ts
// $lib/stores/auth.svelte.ts
interface User {
  id: string;
  email: string;
  name: string;
}

// Encapsulate state in a function that returns getters/setters
export const auth = (() => {
  let user = $state<User | null>(null);
  let loading = $state(false);

  return {
    get user() { return user; },
    get loading() { return loading; },
    get isAuthenticated() { return user !== null; },
    setUser(u: User | null) { user = u; },
    setLoading(l: boolean) { loading = l; },
  };
})();
```

**SSR safety warning:** Global module-level state is shared across all requests on the server. Never mutate global state during SSR rendering — only read it. Mutations should happen client-side only (in `$effect`, event handlers, or after `onMount`). Use SvelteKit's `load` functions to provide per-request data.

### Component Patterns

- **Small, single-responsibility components.** If a component file exceeds ~150 lines, extract sub-components.
- **Props in, callbacks out.** Components receive data via `$props()` and communicate upward via callback props.
- Colocate component styles in `<style>` blocks. Svelte's scoped styles are the default; reach for global styles sparingly.
- Name components in **PascalCase**: `UserCard.svelte`.
- **Don't recreate objects/functions inside templates** — memoize or move creation outside the render path to avoid unnecessary work.
- Use `$state.snapshot(obj)` when you need to pass reactive state to non-Svelte code (e.g., `console.log`, third-party libraries, `JSON.stringify`).

### State Management Hierarchy

1. **Local `$state`** — for state used by a single component.
2. **Passed via `$props`** — for parent-child data flow.
3. **Shared `.svelte.ts` modules** — for state shared across unrelated components (replaces most store use cases).
4. **Svelte stores** (`writable`, `readable`) — still work in Svelte 5 and are NOT deprecated, but runes-based shared state is preferred for new code.
5. **SvelteKit `load` functions** — for route-level data that comes from the server.

Keep state as close to where it's used as possible. Avoid deeply nested global state.

### Data Fetching

- Use SvelteKit's **`load` functions** in `+page.ts` / `+page.server.ts` for route-level data loading. This is the primary data-fetching pattern.
- `+page.server.ts` runs only on the server — use for private credentials, DB access.
- `+page.ts` (universal load) runs on both server and client — use for public API calls.
- **Avoid request waterfalls.** Use `Promise.all` in load functions when fetching multiple independent resources. Don't `await parent()` before fetching your own data unless you actually need the parent data.
- Keep API calls in `$lib/utils/api.ts` — a thin typed wrapper around `fetch`:
  ```ts
  export async function api<T>(
    path: string,
    options?: RequestInit
  ): Promise<T> {
    const res = await fetch(`/api/v1${path}`, {
      headers: { 'Content-Type': 'application/json' },
      ...options,
    });
    if (!res.ok) {
      const error = await res.json().catch(() => ({}));
      throw new ApiError(res.status, error);
    }
    return res.json();
  }
  ```
- Handle loading and error states explicitly — use `{#await}` blocks for inline async or manage state manually with `$state`.
- For error handling in load functions, use SvelteKit's `error()` helper to throw proper HTTP errors that render `+error.svelte`.

### Routing

- File-based routing via SvelteKit's `src/routes/` directory.
- Use **layout groups** `(group)` for shared layouts (e.g., `(app)` for authenticated pages, `(public)` for marketing).
- Use `+error.svelte` pages at every level for graceful error boundaries.
- Use `+layout.ts` / `+layout.server.ts` for data shared across child routes (e.g., auth state, navigation data).
- Use **`+server.ts`** files for API endpoints when you need custom server-side logic beyond load functions.

### Styling

- Pick ONE approach and commit: **Tailwind CSS** or **scoped Svelte styles** (not both mixed randomly).
- If Tailwind: install with SvelteKit using the `@tailwindcss/vite` plugin.
- Design tokens (colors, spacing, fonts) in a central config — either Tailwind config or CSS custom properties.
- For dynamic classes, use array syntax: `class={['base', condition && 'active']}`.

### TypeScript

- Strict mode enabled in `tsconfig.json`.
- Shared types in `$lib/types/`.
- API response types should mirror the backend's response shapes.
- Always define a `Props` interface for component props — don't inline anonymous types.
- Use `import type { Snippet } from 'svelte'` for snippet prop types.
- Use `import type { HTMLButtonAttributes } from 'svelte/elements'` for wrapper components extending native elements.
- SvelteKit auto-generates types for `load` functions — use `import type { PageData } from './$types'` in pages.

### What NOT To Use (Legacy / Deprecated)

These patterns are from Svelte 4 and should NOT be used in new Svelte 5 code:
- `export let` for props → use `$props()`
- `$:` reactive declarations → use `$derived` or `$effect`
- `createEventDispatcher()` → use callback props
- `<slot>` and `let:` → use snippets and `{@render}`
- `on:click` directive syntax → use `onclick` attribute
- `<svelte:component>` → dynamic components work directly now
- `beforeUpdate` / `afterUpdate` → use `$effect` or `$effect.pre`
- `$$props` / `$$restProps` → use rest pattern with `$props()`

---

## Cross-Cutting Concerns

### Authentication

- Passkeys (WebAuthn) as the primary auth method, or magic links as a simpler starting point.
- Session tokens in `HttpOnly` cookies — not localStorage.
- Backend validates sessions via middleware; frontend checks auth state in a root layout `load` function.

### Dev Workflow

- **justfile** (or Makefile) with common commands:
  - `just dev` — start both backend and frontend in watch mode
  - `just build` — production build of both
  - `just test` — run all tests
  - `just check` — cargo check + clippy + svelte-check + tsc
  - `just fmt` — cargo fmt + prettier
- **cargo-watch** for backend hot-reload during development.
- **Prettier** + **eslint** for frontend formatting/linting.
- **Clippy** with `#![warn(clippy::all, clippy::pedantic)]` for Rust linting.

### Environment & Config

- All config via environment variables, loaded from `.env` in dev.
- Backend: typed config struct populated from env vars at startup (fail fast on missing config).
- Frontend: use SvelteKit's `$env/static/public` and `$env/dynamic/private` modules — never expose secrets to the client.

---

## Initial Bootstrap Steps

1. Initialize the monorepo root with a `justfile` and `.env.example`.
2. `cargo init backend` — set up the Rust project with axum, tokio, serde, tracing, thiserror.
3. Create the `AppError` type and a health-check route (`GET /api/v1/health`).
4. `pnpm create svelte@latest frontend` — choose the skeleton project, TypeScript, Prettier, ESLint.
5. Install Tailwind CSS (if chosen) and configure it.
6. Set up the frontend API client in `$lib/utils/api.ts`.
7. Create a root layout that fetches auth state.
8. Wire up `just dev` to run both services concurrently.
9. Verify the frontend can call the backend health endpoint and display the result.

---

## Guiding Principles

- **Compiler is your friend.** Lean on Rust's type system and Svelte's compiler checks. If it compiles, it should be close to correct.
- **Explicit over implicit.** Prefer clear, readable code over clever abstractions.
- **Fewer dependencies.** Add crates/packages deliberately. Evaluate maintenance status and API surface before adding.
- **Progressive enhancement.** Start simple, add complexity only when needed. Don't over-architect before there's a reason.
