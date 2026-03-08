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
│   ├── migrations/   # SQL migrations (sqlx)
│   ├── proto/        # Proto definitions for internal gRPC services
│   └── soul/         # Base SOUL.md (agent identity)
├── frontend/         # SvelteKit PWA
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

> **Token-saving:** All interactive commands use quiet flags to suppress verbose
> output and only print errors. This keeps Claude Code context lean.

```bash
# Backend
cd backend && cargo build -q
cargo test --workspace -q
cargo run -q -p sober-api          # Start API server
cargo run -q --bin sober -- --help  # CLI admin tool (offline ops)
cargo run -q --bin soberctl -- --help # CLI admin tool (runtime ops)

# Linting (errors only)
cargo clippy -q -- -D warnings
cargo fmt --check -q

# Frontend
cd frontend && pnpm install --silent
pnpm dev                            # Dev server on :5173

# Full stack (Docker)
docker compose up -d
```

> **Docker services required for:** integration tests, sqlx compile-time checks,
> anything touching PostgreSQL or Qdrant. Unit tests do **not** require Docker.
> Run `docker compose up -d` before starting integration test work.

---

## Development Rules

### Quiet Output (Token-Saving)

When running build, test, lint, or check commands interactively, **always** use
quiet flags to suppress verbose output and only show errors:

| Tool | Quiet flag | Example |
|------|-----------|---------|
| `cargo build` | `-q` | `cargo build -q` |
| `cargo test` | `-q` | `cargo test -p sober-core -q` |
| `cargo clippy` | `-q` | `cargo clippy -q -- -D warnings` |
| `cargo check` | `-q` | `cargo check -q` |
| `cargo fmt` | `-q` | `cargo fmt --check -q` |
| `cargo doc` | `-q` | `cargo doc -p sober-core --no-deps -q` |
| `cargo audit` | `-q` | `cargo audit -q` |
| `pnpm install` | `--silent` | `pnpm install --silent` |
| `pnpm build` | `--silent` | `pnpm build --silent` |
| `pnpm test` | `--silent` | `pnpm test --silent` |

This applies to all commands in plan verification steps, acceptance criteria,
and ad-hoc development — even if a plan doc doesn't include the `-q` flag.
CI workflows are the exception: they should use normal verbosity for debugging.

### Verify Before Claiming Done

Always verify your work before claiming it is complete:

- After writing or modifying code, run `cargo build -q` to confirm it compiles.
- After implementing a feature or fix, run relevant tests: `cargo test -p <crate> -q`.
- After any code change, run `cargo clippy -q -- -D warnings` on affected crates.
- **Cross-crate changes:** test the modified crate **and** its downstream dependents.
  E.g., changing `sober-core` means also running tests for `sober-crypto`, `sober-auth`, etc.
- For frontend changes, run `pnpm check` and `pnpm test --silent`.
- Never say "this should work" — run it and confirm it does.

### User Interaction

- When you need user input or a decision, **always use the AskUserQuestion tool** — do not
  embed questions in regular output and hope for a response.
- If there is long context the user needs to understand before answering, **print the context
  first**, then follow up with the AskUserQuestion tool.

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
- Each plan lives in its own subfolder named `<number>-<topic>` (e.g., `006-auth/`).
  A plan folder typically contains `design.md` and `plan.md`.
- Each plan document should include: goal, approach, acceptance criteria, and any open questions.
- **Always move the entire plan folder** between directories when changing status —
  never move individual files. Use `git mv docs/plans/pending/006-auth docs/plans/active/006-auth`.

### Git Hooks

- A **pre-commit hook** in `.githooks/` auto-runs `cargo fmt` on staged Rust files.
- After cloning or creating a new worktree, run **`just setup`** to configure hooks.
- The hook formats and re-stages `.rs` files automatically — formatting issues never reach CI.

### Git Workflow & Worktrees

- **Feature development happens exclusively in git worktrees.** Never develop features on the main branch directly.
- **Worktrees live inside the project** at `.worktrees/` (gitignored). Never create sibling directories.
- After creating a worktree, run `just setup` inside it to configure git hooks.
- For every new feature or task:
  1. Create a new worktree: `git worktree add .worktrees/<plan-number>-<feature-name> -b feat/<plan-number>-<feature-name>`
     (e.g., `git worktree add .worktrees/003-auth -b feat/003-auth`)
  2. Do all development in that worktree.
  3. When the feature is ready, create a PR from the feature branch.
  4. When the PR is approved and merged, close the worktree: `git worktree remove .worktrees/<feature-name>`
- Keep worktrees short-lived. One feature per worktree, one worktree per feature.
- The main worktree stays clean — used only for reviews, releases, and non-feature work.

**Branch naming** — prefix matches commit type, and **must include the plan number** when
working on a plan (e.g., `feat/003-feature-name`, `fix/004-bug-description`):

| Prefix | Use case |
|--------|----------|
| `feat/` | New features |
| `fix/` | Bug fixes |
| `refactor/` | Code restructuring |
| `sec/` | Security changes |
| `chore/` | Tooling, deps, CI |

**What can go directly on main** (no worktree/PR required):
- Documentation-only changes (plan docs, CLAUDE.md, README, comments)
- Config file updates (.env.example, CI workflows, justfile)
- Anything that doesn't touch Rust or Svelte source code

**Pull requests:**
- CI must pass before merge.
- Self-merge is OK (solo project) — self-review before merging.
- Use **squash merge** to keep main history linear and clean.
- Never add `Co-Authored-By` or any co-author trailers to commits.
- PRs based on plans must include the **plan number as a prefix** in the title (e.g., "#005: …", "#006: …").

**Plan lifecycle tied to git:**
- When starting a plan, move the entire plan folder from `pending/` to `active/` in the first commit of the feature branch.
- When the PR merges to main, move the entire plan folder from `active/` to `done/`.

### Security

- NEVER commit secrets, keys, or credentials.
- All user input must pass through injection detection before processing.
- Context scopes must never be mixed — verify scope isolation in every PR.
- Crypto operations use only audited crates (ed25519-dalek, aes-gcm, aws-lc-rs).
- Plugin code MUST be sandboxed (wasmtime) before any execution.

### Code Style

- Rust: `cargo fmt --check -q` + `cargo clippy -q -- -D warnings`
- Svelte: `prettier` + `eslint`
- All public functions must have doc comments.
- Error types must be explicit — no `.unwrap()` in library code.
- Use `thiserror` for library errors, `anyhow` only in binaries.

### Architecture

- Each crate has a single responsibility (see @ARCHITECTURE.md crate map).
- Cross-crate dependencies flow downward: api → agent → mind → memory/crypto → core.
- `sober-mind` owns prompt assembly, SOUL.md resolution, access masks, and trait evolution. Agent delegates all prompt construction to `sober-mind`.
- `sober-cli` depends on `sober-core` and `sober-crypto`. It does NOT depend on `sober-api`.
- Never add `sober-api` as a dependency of any other crate.
- `sober-scheduler` and `sober-agent` must NOT depend on each other as crates. They communicate via gRPC at runtime using shared proto definitions.
- Internal service communication uses gRPC over Unix domain sockets (tonic + prost). Proto files live in `backend/proto/`.
- All async code uses `tokio` runtime.
- Database queries use `sqlx` with compile-time checked queries where possible.

### Database Migrations

- Migrations live in `backend/migrations/` and are managed with `sqlx-cli`.
- **Embedded in binary:** Migrations are compiled into the `sober` binary via `sqlx::migrate!()`.
  No SQL files need to be shipped — the binary has everything.
- **Naming:** `YYYYMMDDHHMMSS_description.sql` (generated by `sqlx migrate add <description>`).
- **Creating:** `cd backend && sqlx migrate add <description>` creates a new migration file.
- **Running:** `sober migrate run` (migrations embedded in the binary).
- **Testing:** Always test migrations against a fresh database before committing.
  Docker must be running (`docker compose up -d`).
- **Offline mode for CI:** Run `cargo sqlx prepare` locally after changing queries,
  commit the `.sqlx/` directory. CI builds without needing a database.
- **Deployment:** `install.sh` auto-runs `sober migrate run` on both fresh install
  and upgrades. Migrations are idempotent (sqlx tracks what's already applied).

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

## Versioning

Use **semantic versioning** (semver) for all releases:
- **Major** (`X.0.0`) — breaking changes to public APIs or data formats.
- **Minor** (`0.X.0`) — new features, backward-compatible.
- **Patch** (`0.0.X`) — bug fixes, backward-compatible.

Tag releases as `vX.Y.Z`. Bump version in `Cargo.toml` workspace and `package.json` as appropriate.

**PR version bumps:** Every PR that merges to main must bump the version:
- **Feature PR** (`feat/`) → **minor** bump.
- **Fix PR** (`fix/`) → **patch** bump.

## Commit Convention

```
type(scope): description

feat(agent): add replica spawning protocol
fix(memory): prevent context leak between user scopes
sec(crypto): upgrade to constant-time comparison
docs(arch): update plugin lifecycle diagram
```
