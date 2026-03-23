# Contributing

Thank you for your interest in contributing to Sõber. This page covers everything you need
to go from a fresh clone to a working development environment, along with the conventions
we use across the codebase.

---

## Development Setup

### 1. Clone the repository

```bash
git clone https://github.com/your-org/sober.git
cd sober
```

### 2. Install prerequisites

Building from source requires the following tools:

| Dependency | Version | Notes |
|------------|---------|-------|
| Rust | Latest stable | Install via [rustup](https://rustup.rs) |
| Node.js | 24 | Required for the SvelteKit frontend |
| pnpm | Latest stable | `npm install -g pnpm` |
| protobuf-compiler (`protoc`) | 3.x | Required to compile `.proto` files |
| Docker Engine | 24.0 | Required for integration tests and sqlx checks |
| Docker Compose | v2 | Required for the full dev stack |

**Installing `protoc`:**

```bash
# Debian / Ubuntu
sudo apt install -y protobuf-compiler

# Fedora / RHEL
sudo dnf install -y protobuf-compiler

# macOS
brew install protobuf
```

### 3. Configure the project

Sõber accepts configuration via a `config.toml` file or environment variables with the
`SOBER_` prefix. Both approaches work in development — use whichever you prefer.

**Option A — `config.toml`:**

Copy the example and fill in your values:

```bash
cp config.example.toml config.toml
$EDITOR config.toml
```

**Option B — environment variables:**

Every configuration key has a `SOBER_` prefixed environment variable equivalent. Copy
`.env.example` to `.env` and set the values there:

```bash
cp .env.example .env
$EDITOR .env
```

Environment variables override `config.toml` values when both are present.

At minimum you will need a PostgreSQL connection string, a Qdrant endpoint, and an LLM API
key (or a local Ollama server). See [Configuration](getting-started/configuration.md) for
the full reference.

### 4. Set up git hooks

```bash
just setup
```

This installs the pre-commit hook that auto-formats Rust files before each commit.

### 5. Verify the build

```bash
cd backend && cargo build -q
cd ../frontend && pnpm install --silent && pnpm check
```

---

## Project Structure

```
sober/
├── backend/          # Rust workspace (Cargo)
│   ├── crates/       # Individual library/binary crates
│   ├── migrations/   # SQL migrations (sqlx)
│   └── proto/        # Proto definitions for gRPC services
├── frontend/         # SvelteKit PWA
├── infra/            # Docker, K8s configs
├── docs/
│   ├── book/         # mdBook documentation (this site)
│   ├── plans/        # Internal planning documents
│   ├── rust-patterns.md
│   └── svelte-patterns.md
└── tools/            # Dev scripts, code generators
```

The `backend/` directory is a Cargo workspace. Each crate has a single responsibility — see
[Crate Map](architecture/crates.md) for a full breakdown of what each one owns.

The `frontend/` directory is a SvelteKit application served by the `sober-web` binary in
production and by the Vite dev server during development.

---

## Building

### Backend

```bash
cd backend

# Build all crates
cargo build -q

# Build a single binary
cargo build -q -p sober-api
cargo build -q --bin sober
cargo build -q --bin soberctl
```

### Frontend

```bash
cd frontend
pnpm install --silent
pnpm build --silent    # Production build
pnpm dev               # Dev server on :5173
```

### Full stack (Docker)

```bash
docker compose up -d
```

The Compose file starts PostgreSQL, Qdrant, SearXNG, and all four Sõber processes. Use this
when you need the complete stack running locally or when running integration tests.

---

## Testing

### Unit tests

Unit tests are colocated with source code in `#[cfg(test)]` modules. They do not require
Docker or any external services.

```bash
# All crates
cargo test --workspace -q

# Single crate
cargo test -p sober-core -q
cargo test -p sober-agent -q
```

Or via the justfile:

```bash
just test
```

### Integration tests

Integration tests use `#[sqlx::test]` which provisions a fresh per-test database. Docker
must be running before you execute them.

```bash
docker compose up -d postgres qdrant
cargo test --workspace -q
```

### Frontend tests

```bash
cd frontend
pnpm test --silent
```

---

## Code Style

### Rust

- Format with `cargo fmt` (enforced by the pre-commit hook).
- Lint with `cargo clippy -- -D warnings`. All warnings are treated as errors in CI.
- Use `thiserror` for library crates and `anyhow` for binary crates.
- No `.unwrap()` in library code. Use `.expect("reason")` only when a failure is provably
  impossible.
- All public functions and types must have doc comments.

For detailed patterns and conventions used throughout the Rust codebase, see
[`docs/rust-patterns.md`](https://github.com/your-org/sober/blob/main/docs/rust-patterns.md).

### Svelte / TypeScript

- Format with `prettier` and lint with `eslint`: `pnpm check`.
- Svelte 5 runes only. Do not use legacy Svelte 4 patterns (`export let`, `$:`,
  `createEventDispatcher`, `<slot>`, `on:click`, etc.).
- Strict TypeScript mode is enabled; all types must be explicit.

For detailed frontend patterns, see
[`docs/svelte-patterns.md`](https://github.com/your-org/sober/blob/main/docs/svelte-patterns.md).

---

## Git Workflow

### Branch naming

Use a branch prefix that matches the nature of your change, followed by a short description:

| Prefix | When to use |
|--------|-------------|
| `feat/` | New feature |
| `fix/` | Bug fix |
| `refactor/` | Refactoring without behaviour change |
| `sec/` | Security improvement |
| `chore/` | Tooling, dependencies, CI |

Feature branches that implement a numbered plan include the plan number:

```
feat/003-auth
feat/019-wasm-host-functions
fix/042-memory-leak
```

### Commit convention

```
type(scope): description
```

The scope is the crate or subsystem being changed. Keep the description concise and in the
imperative mood.

**Examples:**

```
feat(agent): add replica spawning protocol
fix(memory): prevent context leak between user scopes
sec(crypto): upgrade to constant-time comparison
docs(arch): update plugin lifecycle diagram
refactor(api): extract auth middleware into shared module
```

### Pull requests

- Open a PR against `main`.
- CI must pass before merging.
- Squash merge. Self-merge is fine once CI is green.
- Keep the PR focused — one logical change per PR.

---

## Database Migrations

Migrations are managed with `sqlx-cli` and embedded in the binary at compile time via
`sqlx::migrate!()`.

**Create a new migration:**

```bash
cd backend
sqlx migrate add <description>
# e.g. sqlx migrate add add_plugin_audit_log
```

This creates a timestamped `.sql` file in `backend/migrations/`. Edit it to add your
schema changes.

**Test against a fresh database:**

Start a local PostgreSQL instance (via Docker Compose) and run:

```bash
cargo run -q --bin sober -- migrate run
```

**Regenerate the sqlx query cache:**

Sõber uses compile-time query verification. The `.sqlx/` cache directory is committed so
that builds work without a live database. After adding or modifying queries, regenerate
the cache with Docker running:

```bash
cd backend
cargo sqlx prepare --workspace
```

Commit the updated `.sqlx/` files alongside your migration and query changes.

---

## Architecture

Before making significant changes it is worth reading the [Architecture](architecture/overview.md)
section of these docs. Key things to know:

- Dependencies flow downward: `api` → `agent` → `mind` → `memory`/`crypto` → `core`.
- `sober-api` must never be imported as a dependency of any other crate.
- `sober-scheduler` and `sober-agent` communicate via gRPC only — there is no crate
  dependency between them.
- Internal service communication uses gRPC over Unix domain sockets (tonic + prost).
  Proto definitions live in `backend/proto/`.

The [Crate Map](architecture/crates.md) lists every crate with a one-line summary of its
responsibility.
