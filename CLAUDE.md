# CLAUDE.md — Sõber Project Instructions

## Project Overview

Sõber ("friend" in Estonian) is a self-evolving AI agent system built with Rust (backend)
and Svelte 5 (frontend). See @ARCHITECTURE.md for full system design.

## Guiding Principles

- **Compiler is your friend.** Lean on Rust's type system and Svelte's compiler checks.
- **Explicit over implicit.** Prefer clear, readable code over clever abstractions.
- **Fewer dependencies.** Add crates/packages deliberately. Evaluate maintenance status and API surface.
- **Progressive enhancement.** Start simple, add complexity only when needed.

## Best Practices & Patterns

- Rust: @docs/rust-patterns.md
- Svelte 5: @docs/svelte-patterns.md

## Repository Structure

```
sober/
├── backend/          # Rust workspace (Cargo)
│   ├── crates/       # Individual library/binary crates
│   │   └── sober-cli/ # CLI: `sober` (offline) + `soberctl` (runtime)
│   ├── migrations/   # SQL migrations (sqlx)
│   ├── proto/        # Proto definitions for internal gRPC services
│   └── soul/         # Base SOUL.md (agent identity)
├── frontend/         # SvelteKit PWA
├── infra/            # Docker, K8s configs
├── docs/
│   ├── plans/        # pending/ → active/ → done/
│   ├── rust-patterns.md
│   └── svelte-patterns.md
└── tools/            # Dev scripts, code generators
```

## Build & Run

**Prerequisites:** Node.js 24, Rust (latest stable), pnpm, Docker

```bash
# Backend — build & test
cd backend && cargo build -q
cargo test --workspace -q
cargo test -p sober-core -q         # Single crate

# Backend — run
cargo run -q -p sober-api           # API server
cargo run -q -p sober-web           # Web server (frontend + reverse proxy)
cargo run -q --bin sober -- --help   # CLI admin tool (offline ops)
cargo run -q --bin soberctl -- --help # CLI admin tool (runtime ops)

# Backend — lint & format
cargo clippy -q -- -D warnings
cargo fmt --check -q
cargo audit -q

# Frontend
cd frontend && pnpm install --silent
pnpm dev                             # Dev server on :5173
pnpm build --silent                  # Production build
pnpm check                           # Svelte/TS type check
pnpm test --silent                   # Tests

# Full stack (Docker)
docker compose up -d

# Dev workflow (justfile)
just dev                             # Start dev servers
just build                           # Build all
just test                            # Run all tests
just check                           # Lint + type check
just fmt                             # Format all
just setup                           # Configure git hooks
```

> Docker required for: integration tests, sqlx compile-time checks, PostgreSQL/Qdrant.
> Unit tests do **not** require Docker.

---

## Development Rules

### Quiet Output

**Always** use quiet flags (`-q`, `--silent`) for all interactive commands. This saves context tokens.
CI workflows are the exception — use normal verbosity for debugging.

### Verify Before Claiming Done

- After code changes: `cargo build -q`, `cargo clippy -q -- -D warnings`, `cargo test -p <crate> -q`.
- **Cross-crate changes:** test modified crate **and** downstream dependents. `cargo test --workspace -q` when in doubt.
- Frontend: `pnpm check` and `pnpm test --silent`.
- Never say "this should work" — run it and confirm.

### User Interaction

- **Always use AskUserQuestion tool** for user input or decisions — don't embed questions in output.

### Dependencies

- Use **latest stable versions**. Look up versions — don't guess.
- Evaluate before adding: maintenance status, download count, `unsafe` usage.

### Code Navigation

- Prefer **LSP** for navigation (go-to-definition, find references, rename).

### Planning & Documentation

- Plans live in `docs/plans/` subfolders: `pending/` → `active/` → `done/`.
- Each plan: `<number>-<topic>/` folder with `design.md` and `plan.md`.
- **Move entire folders** between directories. Use `git mv`.

### Git Hooks

- Pre-commit hook in `.githooks/` auto-formats Rust files.
- Run **`just setup`** after cloning or creating a worktree.

### Git Workflow & Worktrees

- **Feature work happens in worktrees only.** Never on main directly.
- Worktrees at `.worktrees/` (gitignored). Run `just setup` inside after creating.
- Workflow: `git worktree add .worktrees/<plan>-<name> -b feat/<plan>-<name>` → develop → PR → merge → `git worktree remove`.
- **Always check existing worktrees** (`git worktree list`) before starting work.
- Copy `.env` to worktree directory after creating it.

**Branch prefixes** (include plan number, e.g., `feat/003-auth`): `feat/`, `fix/`, `refactor/`, `sec/`, `chore/`.

**Direct to main** (no worktree): docs-only, config changes, non-Rust/Svelte changes.

**Pull requests:**
- CI must pass. Squash merge. Self-merge OK.
- Never add `Co-Authored-By` trailers.
- Plan PRs: prefix title with plan number (e.g., "#005: …").
- **Never merge PRs unrelated to current worktree.**

**Plan lifecycle:** Move folder `pending/` → `active/` in first commit, `active/` → `done/` in last commit.

### Security

- Never commit secrets. Injection detection on all user input.
- Crypto: only audited crates (ed25519-dalek, aes-gcm, aws-lc-rs).
- Plugins sandboxed (wasmtime) before execution.

### Code Style

- Rust: `cargo fmt` + `cargo clippy -D warnings`. `thiserror` for libs, `anyhow` for bins.
- Svelte: `prettier` + `eslint`.
- Public functions need doc comments. No `.unwrap()` in library code.

### Architecture

- Each crate has single responsibility. See @ARCHITECTURE.md crate map.
- Dependencies flow downward: api → agent → mind → memory/crypto → core.
- `sober-mind` owns prompt assembly. Agent delegates prompt construction to mind.
- `sober-cli` depends on core + crypto. NOT on api.
- Never add `sober-api` as a dependency of any other crate.
- `sober-scheduler` and `sober-agent` communicate via gRPC only — no crate dependency.
- Internal comms: gRPC/UDS (tonic + prost). Protos in `backend/proto/`.

### Database Migrations

- Managed with `sqlx-cli`. Embedded in binary via `sqlx::migrate!()`.
- Create: `cd backend && sqlx migrate add <description>`.
- Run: `sober migrate run`. Test against fresh DB (Docker required).
- CI offline mode: `cargo sqlx prepare` → commit `.sqlx/` directory.

### Testing

- Unit tests colocated in `#[cfg(test)]` modules.
- Integration tests use `#[sqlx::test]` with per-test DB.
- Security-critical code: property-based testing (proptest).

### Authentication

- Passkeys (WebAuthn) primary. Sessions in `HttpOnly` cookies.
- Backend middleware validates. Frontend checks in root layout `load`.

### Environment & Config

- All config via env vars, `.env` in dev. Backend: typed config struct, fail fast.
- Frontend: `$env/static/public` and `$env/dynamic/private`. Never expose secrets to client.

---

## Versioning

**PR version bumps (STRICT):** Every PR bumps the version:
- `feat/` → **MINOR** (`0.X.0`). Never patch for features.
- `fix/` → **patch** (`0.0.X`).
- Bump ALL affected crate `Cargo.toml` versions.

## Commit Convention

```
type(scope): description

feat(agent): add replica spawning protocol
fix(memory): prevent context leak between user scopes
sec(crypto): upgrade to constant-time comparison
docs(arch): update plugin lifecycle diagram
```
