# 002 — Project Skeleton: Implementation Plan

**Date:** 2026-03-06
**Status:** Pending
**Design:** [design.md](./design.md)

This plan bootstraps the Sober repository from docs-only into a buildable,
testable project skeleton. Every step is atomic and verifiable.

---

## Steps

### 1. Create the Cargo workspace

Create `backend/Cargo.toml` with:
- `[workspace]` with `members = ["crates/*"]` and `resolver = "2"`
- `[workspace.package]` with `edition = "2024"`
- `[workspace.dependencies]` pinning: serde, tokio, thiserror, anyhow, tracing,
  tracing-subscriber, uuid, sqlx, tonic, prost
- `[workspace.build-dependencies]` pinning: tonic-build
- `[profile.release]` with `overflow-checks = true`

- [ ] File exists and `cargo metadata` succeeds from `backend/`

### 2. Create stub crates

Create all eleven crates under `backend/crates/`:

**Library crates** (each has `Cargo.toml` + `src/lib.rs`):
- `sober-core` — no internal dependencies
- `sober-crypto` — depends on sober-core
- `sober-auth` — depends on sober-crypto, sober-core
- `sober-memory` — depends on sober-core
- `sober-llm` — depends on sober-core
- `sober-mcp` — depends on sober-core
- `sober-mind` — depends on sober-memory, sober-crypto, sober-auth, sober-core
- `sober-agent` — depends on sober-mind, sober-mcp, sober-llm, sober-memory, sober-core

**Binary crates** (each has `Cargo.toml` + `src/main.rs`):
- `sober-api` — `[[bin]] name = "sober-api"`, depends on sober-agent, sober-auth, sober-core
- `sober-scheduler` — `[[bin]] name = "sober-scheduler"`, depends on sober-core, sober-crypto
- `sober-cli` — two `[[bin]]` sections (`sober` and `soberctl`), depends on sober-core only

Each `Cargo.toml` inherits `edition.workspace = true` and uses `dep.workspace = true`
for shared dependencies where applicable.

Library `lib.rs` files contain a doc comment describing the crate's purpose.
Binary `main.rs` files contain a minimal `fn main()` with a placeholder print.

- [ ] All eleven `backend/crates/*/Cargo.toml` files exist
- [ ] All eleven `backend/crates/*/src/{lib,main}.rs` files exist
- [ ] sober-cli has two `[[bin]]` entries and corresponding source files

### 3. Create migrations directory

Create `backend/migrations/` as an empty directory with a `.gitkeep` file.

- [ ] Directory exists

### 4. Create `.env.example`

Document all environment variables at the project root. Group by category:
database, redis, qdrant, API server, LLM providers, logging, frontend.
Include comments explaining each variable. No actual secrets.

- [ ] File exists at project root

### 5. Create `docker-compose.yml`

Define four services at the project root:
- `postgres` — postgres:17, port 5432, with health check
- `qdrant` — qdrant/qdrant, ports 6333/6334
- `redis` — redis:7, port 6379, with health check
- `searxng` — searxng/searxng, port 8080

Use named volumes for persistent data. Set environment variables from `.env`.

- [ ] File exists at project root
- [ ] `docker compose config` validates without error

### 6. Create `justfile`

Define commands at the project root:
- `dev` — start backend (cargo-watch) and frontend (pnpm dev) concurrently
- `build` — cargo build --release + pnpm build
- `test` — cargo test --workspace + pnpm test
- `check` — cargo check + cargo clippy + pnpm check
- `fmt` — cargo fmt + pnpm format
- `lint` — cargo clippy -- -D warnings + pnpm lint
- `audit` — cargo audit

- [ ] File exists at project root
- [ ] All commands are defined (syntactically valid)

### 7. Create CI workflow

Create `.github/workflows/ci.yml`:
- Triggers on push and pull_request
- Jobs: backend (fmt, clippy, test, audit) and frontend (install, check)
- Caches Cargo registry and target directory
- Uses latest stable Rust and Node.js 24

- [ ] File exists
- [ ] YAML is valid

### 8. Scaffold the frontend

Initialize the SvelteKit project in `frontend/`:
- TypeScript, skeleton project
- Install Tailwind CSS via `@tailwindcss/vite`
- Configure strict TypeScript
- Create `src/lib/utils/api.ts` with a typed fetch wrapper
- Create `src/lib/types/` directory for shared types

- [ ] `pnpm install` succeeds
- [ ] `pnpm check` succeeds

### 9. Create `.gitignore` and `.dockerignore`

`.gitignore` at project root:
- `target/`, `node_modules/`, `.env`, `build/`, `.svelte-kit/`
- OS files: `.DS_Store`, `Thumbs.db`
- Editor files: `.idea/`, `*.swp`

`.dockerignore` at project root:
- `target/`, `node_modules/`, `.git/`, `.env`

- [ ] Both files exist at project root

### 10. Create `shared/` directory

Create `shared/proto/` directory structure for internal gRPC service definitions:
- `shared/proto/sober/agent/v1/.gitkeep`
- `shared/proto/sober/scheduler/v1/.gitkeep`

Proto files will be populated in phase 012 (scheduler/IPC).

- [ ] Directory structure exists

---

## Acceptance Criteria

All of the following must pass before this plan is considered complete:

- [ ] `cargo build` succeeds in `backend/`
- [ ] `cargo test --workspace` succeeds in `backend/` (compiles, zero tests is acceptable)
- [ ] `cargo clippy -- -D warnings` produces no warnings in `backend/`
- [ ] `pnpm install && pnpm check` succeeds in `frontend/`
- [ ] `docker compose config` validates without error at project root
- [ ] All `justfile` commands are defined and syntactically valid
- [ ] CI workflow YAML is valid
- [ ] No secrets are committed (no `.env` file, only `.env.example`)
