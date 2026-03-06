# CLAUDE.md — Sõber Project Instructions

## Project Overview

Sõber ("friend" in Estonian) is a self-evolving AI agent system built with Rust (backend)
and Svelte 5 (frontend). See @ARCHITECTURE.md for full system design.

## Best Practices & Patterns

- Rust: @docs/rust-patterns.md
- Svelte 5: @docs/svelte-patterns.md
- Bootstrap guide & guiding principles: @CLAUDE_CODE_BOOTSTRAP.md

## Repository Structure

```
sober/
├── backend/          # Rust workspace (Cargo)
│   ├── crates/       # Individual library/binary crates
│   │   └── sober-cli/ # CLI: `sober` (offline) + `soberctl` (runtime)
│   └── migrations/   # SQL migrations (sqlx)
├── frontend/         # SvelteKit PWA
├── shared/           # Proto definitions for internal gRPC services
├── infra/            # Docker, K8s configs
├── docs/
│   ├── plans/        # Planning documents
│   │   ├── pending/  # Not yet started
│   │   ├── active/   # Currently in progress
│   │   └── done/     # Completed
│   ├── rust-patterns.md
│   └── svelte-patterns.md
└── tools/            # Dev scripts, code generators
```

## Build & Run

**Prerequisites:** Node.js 24, Rust (latest stable), pnpm, Docker

```bash
# Backend
cd backend && cargo build
cargo test --workspace
cargo run -p sober-api          # Start API server
cargo run --bin sober -- --help # CLI admin tool (offline ops)
cargo run --bin soberctl -- --help # CLI admin tool (runtime ops)

# Frontend
cd frontend && pnpm install
pnpm dev                        # Dev server on :5173

# Full stack (Docker)
docker compose up -d
```

---

## Development Rules

### Dependencies

- Always use the **latest stable versions** of all dependencies.
- If unsure about the current version of a crate or npm package, **look it up** — do not guess. Check crates.io, npmjs.com, or run `cargo search` / `pnpm info`.
- Evaluate before adding: check maintenance status, download count, and `unsafe` usage.
- Run `cargo audit` regularly to catch known vulnerabilities.

### Code Navigation

- Always prefer **LSP** for code navigation (go-to-definition, find references, rename, etc.).
- Do not grep or manually search when LSP can provide accurate, type-aware results.

### Planning & Documentation

- All plan documents live in `docs/plans/` with three subdirectories:
  - `docs/plans/pending/` — planned but not yet started
  - `docs/plans/active/` — currently being worked on
  - `docs/plans/done/` — completed (move here when finished)
- Each plan document should include: goal, approach, acceptance criteria, and any open questions.
- Move plan files between directories as their status changes.

### Git Workflow & Worktrees

- **Feature development happens exclusively in git worktrees.** Never develop features on the main branch directly.
- For every new feature or task:
  1. Create a new worktree: `git worktree add ../sober-<feature-name> -b feat/<feature-name>`
  2. Do all development in that worktree.
  3. When the feature is ready, create a PR from the feature branch.
  4. When the PR is approved and merged, close the worktree: `git worktree remove ../sober-<feature-name>`
- Keep worktrees short-lived. One feature per worktree, one worktree per feature.
- The main worktree stays clean — used only for reviews, releases, and non-feature work.

### Security

- NEVER commit secrets, keys, or credentials.
- All user input must pass through injection detection before processing.
- Context scopes must never be mixed — verify scope isolation in every PR.
- Crypto operations use only audited crates (ed25519-dalek, aes-gcm, aws-lc-rs).
- Plugin code MUST be sandboxed (wasmtime) before any execution.

### Code Style

- Rust: `cargo fmt` + `cargo clippy -- -D warnings`
- Svelte: `prettier` + `eslint`
- All public functions must have doc comments.
- Error types must be explicit — no `.unwrap()` in library code.
- Use `thiserror` for library errors, `anyhow` only in binaries.

### Architecture

- Each crate has a single responsibility (see @ARCHITECTURE.md crate map).
- Cross-crate dependencies flow downward: api → agent → memory/crypto → core.
- `sober-cli` depends on `sober-core` and `sober-crypto`. It does NOT depend on `sober-api`.
- Never add `sober-api` as a dependency of any other crate.
- `sober-scheduler` and `sober-agent` must NOT depend on each other as crates. They communicate via gRPC at runtime using shared proto definitions.
- Internal service communication uses gRPC over Unix domain sockets (tonic + prost). Proto files live in `shared/proto/`.
- All async code uses `tokio` runtime.
- Database queries use `sqlx` with compile-time checked queries where possible.

### Memory System

- Binary Context Format (BCF) is the canonical storage format.
- Always load minimal context — never load full user history for a single query.
- Vector operations go through sober-memory, never call Qdrant directly from other crates.
- Pruning runs automatically; importance scores decay over time.

### Plugin/Skill Development

- Plugins implement the `SoberPlugin` trait.
- All plugins must declare capabilities upfront.
- Predictable logic should be compiled to WASM, not run through LLM loop.
- MCP compatibility is required for external tool interop.

### Testing

- Unit tests in each crate's `src/` (Rust convention).
- Integration tests in `tests/` directories.
- Security-critical code requires property-based testing (proptest).
- Plugin audit pipeline must have dedicated test suite.

---

## Key Dependencies

### Rust
- `axum` — HTTP framework
- `tokio` — Async runtime
- `sqlx` — Database (PostgreSQL 17)
- `qdrant-client` — Vector database
- `ed25519-dalek` — Signing
- `aes-gcm` — Symmetric encryption
- `aws-lc-rs` — Crypto backend (replaces ring)
- `argon2` — Password hashing
- `serde` / `bincode` — Serialization
- `zstd` — Compression
- `wasmtime` — Plugin sandbox
- `openidconnect` — OIDC client
- `webauthn-rs` — Passkey/FIDO2
- `clap` — CLI argument parsing (derive)
- `tracing` — Structured logging
- `thiserror` — Error types
- `tonic` — gRPC framework (internal service communication)
- `prost` — Protocol Buffers codegen
- `rustls` — TLS (pure Rust)

### Frontend
- `@sveltejs/kit` — Framework (Svelte 5 runes only)
- `tailwindcss` — Styling

## MCP Servers for Development

Recommended MCP integrations to install for development workflow:
- GitHub MCP — PR reviews, issue management
- Filesystem MCP — Local file operations
- PostgreSQL MCP — Direct DB inspection
- Docker MCP — Container management
- Memory MCP — Persistent dev context

## Commit Convention

```
type(scope): description

feat(agent): add replica spawning protocol
fix(memory): prevent context leak between user scopes
sec(crypto): upgrade to constant-time comparison
docs(arch): update plugin lifecycle diagram
```
