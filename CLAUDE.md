# CLAUDE.md — Sõber Project Instructions

See @ARCHITECTURE.md for system design.

### Best Practices & Patterns

- Rust: @docs/rust-patterns.md
- Svelte 5: @docs/svelte-patterns.md

## Build & Run

**Prerequisites:** Node.js 24, Rust (latest stable), pnpm, Docker

```bash
# Backend
cd backend && cargo build -q
cargo test --workspace -q              # All crates
cargo test -p sober-core -q            # Single crate
cargo clippy -q -- -D warnings
cargo fmt --check -q

# Run
cargo run -q -p sober-api              # API server
cargo run -q -p sober-web              # Web server (frontend + reverse proxy)
cargo run -q --bin sober -- --help     # Unified CLI

# Frontend
cd frontend && pnpm install --silent
pnpm build && pnpm check && pnpm test --silent

# Justfile shortcuts
just dev | build | test | check | fmt | setup
```

> Docker required for: integration tests, sqlx compile-time checks, PostgreSQL/Qdrant. Unit tests do not require Docker.

## Development Rules

- **Quiet output.** Always use `-q` / `--silent` for interactive commands (cargo, pnpm, docker). CI uses normal verbosity for debugging.
- **Verify before done.** Build → clippy → test. Cross-crate: `cargo test --workspace -q`. Frontend: `pnpm check` + `pnpm test --silent`. Never say "this should work" — run it.
- **Code style.** Rust: `cargo fmt` + `cargo clippy -D warnings`. Svelte: `prettier` + `eslint`.
- **User interaction.** Always use AskUserQuestion tool — don't embed questions in output.
- **Dependencies.** Latest stable versions. Look up — don't guess. Evaluate maintenance status.
- **Code navigation.** Prefer LSP (Rust + TypeScript) for go-to-definition, find references, and rename. Use over grep/glob when possible.
- **No `.unwrap()` in library code.** `thiserror` for libs, `anyhow` for bins. Public functions need doc comments.
- **Confirm before implementing.** After plan approval, ask the user before starting — don't auto-start.
- **Rebuild Docker after changes.** Run `docker compose up -d --build -q` after code changes — don't wait to be asked. Always use `-q` to suppress build output.
- **`context_modifying` on tools.** Only set `context_modifying: true` on tools that mutate state (memory writes, file edits). Setting it on read-only tools triggers context rebuild which loses `reasoning_content` from DB, breaking thinking-enabled models.
- **Update docs with code.** When architecture or functionality changes, update `ARCHITECTURE.md` and user-facing documentation (mdBook site in `docs/`) in the same PR.

## Architecture Guardrails

- Deps flow downward: `api → agent → mind → memory/crypto → core`.
- Never add `sober-api` as a dependency of any other crate.
- `sober-scheduler` ↔ `sober-agent`: gRPC only — no crate dependency.
- Never commit secrets. Only audited crypto crates (ed25519-dalek, aes-gcm, aws-lc-rs).

## Database Migrations

```bash
cd backend && sqlx migrate add <description>  # Create
sober migrate run                              # Run (Docker required)
cargo sqlx prepare                             # CI offline mode → commit .sqlx/
```

## Git Workflow

- **Feature work in worktrees only.** Never commit to main directly.
- Worktrees at `.worktrees/`. Run `just setup` inside after creating. Copy `.env`.
- `git worktree add .worktrees/<plan>-<name> -b feat/<plan>-<name>` → develop → PR → merge → remove.
- Check `git worktree list` before starting work.
- **All commands run from worktree dir.** Never cd back to main repo — Docker uses CWD as build context.
- **Direct to main** (no worktree): docs-only, config changes, non-Rust/Svelte changes.

**Branches:** include plan number — `feat/003-auth`, `fix/012-leak`. Prefixes: `feat/`, `fix/`, `refactor/`, `sec/`, `chore/`.

**PRs:** CI must pass. Squash merge. Self-merge OK. Never add `Co-Authored-By` trailers. Plan PRs: prefix title with plan number (e.g., "#005: …"). Never merge PRs unrelated to current worktree.

## Plans

- Location: `docs/plans/` with subfolders: `pending/`, `active/`, `done/`.
- Each plan: `<number>-<topic>/` folder containing `design.md` and `plan.md`.
- Move entire folders between directories with `git mv`.
- **Lifecycle:** `pending/` → `active/` in first commit, `active/` → `done/` in last commit.

## Versioning & Commits

**PR version bumps (STRICT):** Every PR bumps the version:
- `feat/` → **MINOR** (`0.X.0`). Never patch for features.
- `fix/` → **patch** (`0.0.X`).
- Bump ALL affected crate `Cargo.toml` versions.

**Commit convention:**

```
type(scope): description

feat(agent): add replica spawning protocol
fix(memory): prevent context leak between user scopes
sec(crypto): upgrade to constant-time comparison
docs(arch): update plugin lifecycle diagram
```
