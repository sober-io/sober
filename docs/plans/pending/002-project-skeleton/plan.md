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

Create all twelve crates under `backend/crates/`:

**Library crates** (each has `Cargo.toml` + `src/lib.rs`):
- `sober-core` — no internal dependencies
- `sober-crypto` — depends on sober-core
- `sober-auth` — depends on sober-crypto, sober-core
- `sober-memory` — depends on sober-core
- `sober-llm` — depends on sober-core
- `sober-sandbox` — depends on sober-core; external dep: `bwrap` process sandboxing
- `sober-mcp` — depends on sober-sandbox, sober-core
- `sober-mind` — depends on sober-memory, sober-crypto, sober-auth, sober-core

**Binary crates** (each has `Cargo.toml` + `src/main.rs`):
- `sober-agent` — `[[bin]] name = "sober-agent"`, gRPC server (tonic), depends on sober-mind, sober-mcp, sober-sandbox, sober-llm, sober-memory, sober-core
- `sober-api` — `[[bin]] name = "sober-api"`, depends on sober-auth, sober-core
- `sober-scheduler` — `[[bin]] name = "sober-scheduler"`, depends on sober-core, sober-crypto
- `sober-cli` — two `[[bin]]` sections (`sober` and `soberctl`), depends on sober-crypto, sober-core

Each `Cargo.toml` inherits `edition.workspace = true` and uses `dep.workspace = true`
for shared dependencies where applicable.

Library `lib.rs` files contain a doc comment describing the crate's purpose.
Binary `main.rs` files contain a minimal `fn main()` with a placeholder print.

- [ ] All twelve `backend/crates/*/Cargo.toml` files exist
- [ ] All twelve `backend/crates/*/src/{lib,main}.rs` files exist
- [ ] sober-cli has two `[[bin]]` entries and corresponding source files

### 3. Create migrations directory

Create `backend/migrations/` as an empty directory with a `.gitkeep` file.

- [ ] Directory exists

### 4. Create `.env.example`

Document all environment variables at the project root. Group by category:
database, qdrant, API server, LLM, logging, frontend. Use canonical
var names: `LLM_BASE_URL`, `LLM_API_KEY`, `LLM_MODEL`, `HOST`, `PORT`,
`RUST_LOG`. No provider-specific keys (no `ANTHROPIC_API_KEY` etc.).
Include comments explaining each variable. No actual secrets.

- [ ] File exists at project root

### 5. Create `docker-compose.yml`

Define four services at the project root:
- `postgres` — postgres:17, port 5432, with health check
- `qdrant` — qdrant/qdrant, ports 6333/6334
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

### 10. Add PostgreSQL MCP config

Create `.claude/mcp.json` with `crystaldba/postgres-mcp` pointing at the Docker
Compose dev database:

```json
{
  "mcpServers": {
    "postgres": {
      "command": "npx",
      "args": [
        "-y", "@crystaldba/postgres-mcp",
        "postgresql://sober:sober@localhost:5432/sober"
      ]
    }
  }
}
```

This gives Claude Code direct DB access for schema inspection, query debugging,
and data exploration during development. The connection string matches the
`postgres` service in `docker-compose.yml`.

- [ ] File exists at `.claude/mcp.json`
- [ ] JSON is valid

### 11. Create shared test infrastructure in `sober-core`

Add a `test_utils` module in `sober-core` behind a `#[cfg(feature = "test-utils")]` feature
flag. This module provides shared test helpers for all downstream crates:

- `MockLlmEngine` — implements `LlmEngine` trait, returns configurable canned responses
  (text-only, tool calls, streaming chunks). Used by agent integration tests.
- `test_db()` — creates a test PostgreSQL connection pool using `DATABASE_URL` from env,
  runs migrations, returns `PgPool`. Wraps each test in a transaction that rolls back.
- `test_config()` — returns an `AppConfig` populated with test defaults.
- `MockGrpcServer` — minimal tonic server that implements `AgentService` with canned
  responses for testing gRPC clients.

Each downstream crate can depend on `sober-core` with `features = ["test-utils"]` in
`[dev-dependencies]`.

For bwrap-dependent tests (plan 008): gate behind `#[cfg(target_os = "linux")]` and skip
in CI if namespaces are not available (check with `bwrap --version` in a setup step).

- [ ] `test_utils` module exists behind feature flag
- [ ] Mock types compile (actual implementations filled in by plans 003+)

### 12. Create `shared/proto/` directory with stub protos

Create `shared/proto/` directory structure for internal gRPC service definitions
with minimal stub proto files:

- `shared/proto/sober/agent/v1/agent.proto` — stub with `AgentService` and a `Health` RPC
- `shared/proto/sober/scheduler/v1/scheduler.proto` — stub with `SchedulerService` and a `Health` RPC

These stubs enable `tonic-build` in `sober-agent` and `sober-scheduler` from the start.
Full RPC methods will be added as features are implemented.

- [ ] Proto files exist and are syntactically valid
- [ ] `sober-agent` and `sober-scheduler` build scripts can compile the protos

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
