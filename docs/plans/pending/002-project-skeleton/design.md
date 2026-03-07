# 002 — Project Skeleton

**Date:** 2026-03-06
**Status:** Pending

This document records the design decisions for bootstrapping the Sober project from a
docs-only repository into a buildable, testable project skeleton.

---

## Cargo Workspace Layout

The backend uses a Cargo workspace rooted at `backend/Cargo.toml`:

```toml
[workspace]
members = ["crates/*"]
resolver = "2"
```

All crates live under `backend/crates/`. This keeps the workspace flat and makes
adding new crates trivial — just create a directory under `crates/` and it is
automatically included.

**Rust edition:** 2024 for all crates. Set at the workspace level via
`[workspace.package]` and inherited by each crate with `edition.workspace = true`.

---

## Crate Inventory

Twelve stub crates, matching the architecture document:

| Crate | Type | Internal Dependencies |
|-------|------|-----------------------|
| `sober-core` | library | none |
| `sober-crypto` | library | sober-core |
| `sober-auth` | library | sober-crypto, sober-core |
| `sober-memory` | library | sober-core |
| `sober-llm` | library | sober-core |
| `sober-sandbox` | library | sober-core |
| `sober-mcp` | library | sober-sandbox, sober-core |
| `sober-mind` | library | sober-memory, sober-crypto, sober-core |
| `sober-agent` | binary (gRPC) | sober-mind, sober-mcp, sober-sandbox, sober-llm, sober-memory, sober-core |
| `sober-api` | binary | sober-auth, sober-core |
| `sober-scheduler` | binary | sober-crypto, sober-core |
| `sober-cli` | binary | sober-crypto, sober-core |

Binary crates have `main.rs`; library crates have `lib.rs`. `sober-cli` produces
two binaries (`sober` and `soberctl`) via `[[bin]]` sections.

Dependency flow is strictly downward: `agent -> {mind, mcp, sandbox, llm, memory} -> core`
and `api -> auth -> crypto -> core`. `sober-api` and `sober-scheduler` communicate
with `sober-agent` via gRPC/UDS at runtime, not as crate dependencies. Proto
definitions live in `shared/proto/`.

---

## Workspace-Level Dependencies

Common dependencies are pinned once in `[workspace.dependencies]` and inherited by
crates via `dep.workspace = true`. This prevents version drift across the workspace.

Initial workspace dependencies:

- `serde` (with `derive` feature)
- `tokio` (with `full` feature)
- `thiserror`
- `anyhow`
- `tracing`
- `tracing-subscriber`
- `uuid` (with `v7` and `serde` features)
- `sqlx` (with `runtime-tokio`, `tls-rustls-aws-lc-rs`, `postgres` features)

**TLS and crypto backends:** `rustls` is the default TLS implementation (pure Rust,
no OpenSSL dependency). `aws-lc-rs` is the cryptographic backend, replacing `ring`
which the ecosystem is migrating away from.

---

## Workspace Profile Settings

```toml
[profile.release]
overflow-checks = true
```

Integer overflow checks remain enabled in release builds. The performance cost is
negligible for this application, and silent overflow is a security risk.

---

## Task Runner

`justfile` (using [just](https://github.com/casey/just)) is the project task runner.
Chosen over Makefile for cleaner syntax, better error messages, and cross-platform
support.

Commands:

| Command | Description |
|---------|-------------|
| `just dev` | Start backend and frontend in watch mode |
| `just build` | Production build of both backend and frontend |
| `just test` | Run all tests (backend + frontend) |
| `just check` | cargo check + clippy + svelte-check + tsc |
| `just fmt` | cargo fmt + prettier |
| `just lint` | cargo clippy + eslint |
| `just audit` | cargo audit |

---

## Environment Configuration

An `.env.example` file documents all environment variables with sensible defaults
for local development. Categories:

- **Database:** `DATABASE_URL`, `DATABASE_MAX_CONNECTIONS`
- **Qdrant:** `QDRANT_URL`
- **API server:** `HOST`, `PORT`, `ADMIN_SOCKET_PATH`
- **LLM:** `LLM_BASE_URL`, `LLM_API_KEY`, `LLM_MODEL`
- **Logging:** `RUST_LOG`
- **Frontend:** `PUBLIC_API_URL`

Secrets are never committed. The `.env` file is gitignored.

---

## Docker Compose (Development)

`docker-compose.yml` provides the development infrastructure. No application
containers — the backend and frontend run natively during development.

Services:

| Service | Image | Port | Purpose |
|---------|-------|------|---------|
| `postgres` | `postgres:17` | 5432 | Primary relational database |
| `qdrant` | `qdrant/qdrant` | 6333/6334 | Vector storage and search |
| *(no Redis in v1 — using moka in-memory cache)* | | | |
| `searxng` | `searxng/searxng` | 8080 | Web search for agent |

No reverse proxy (Caddy, nginx) in the dev stack. The backend serves the API
directly, and the frontend dev server handles its own requests.

---

## CI Pipeline

`.github/workflows/ci.yml` runs on every push and pull request:

1. `cargo fmt --check` — formatting consistency
2. `cargo clippy -- -D warnings` — lint with warnings as errors
3. `cargo test --workspace` — all tests
4. `cargo audit` — dependency vulnerability scan
5. `pnpm install && pnpm check` — frontend type checking

The pipeline uses caching for `~/.cargo` and `target/` to speed up builds.

---

## Frontend Scaffold

The frontend is a SvelteKit application:

- Created with `pnpm create svelte@latest` (skeleton project, TypeScript)
- Tailwind CSS installed via the `@tailwindcss/vite` plugin
- Strict TypeScript mode enabled
- Svelte 5 runes only — no legacy patterns

---

## Shared Test Infrastructure

`sober-core` provides a `test_utils` module behind a `test-utils` feature flag. This
gives all downstream crates access to shared test helpers without polluting production
builds:

- Test database pool (connects to test DB, runs migrations, transaction-per-test)
- Test config with sensible defaults
- Mock LLM engine — deferred to plan 008 (sober-llm) when the `LlmEngine` trait exists
- Mock gRPC server — deferred to plan 012 (sober-agent) when the gRPC service is defined

Downstream crates add `sober-core = { ..., features = ["test-utils"] }` in
`[dev-dependencies]`. Actual mock implementations are filled in as each crate
is implemented (plans 003+).

For Linux-specific tests (bwrap sandboxing in plan 009): use `#[cfg(target_os = "linux")]`
and detect bwrap availability at test runtime.

---

## Shared Directory

`shared/proto/` holds gRPC service definitions (Protocol Buffers) for internal
inter-process communication. `sober-api` and `sober-scheduler` use these proto
files to generate gRPC client stubs for calling `sober-agent`. `sober-agent`
uses them to generate its gRPC server implementation.

Proto file layout:
- `shared/proto/sober/agent/v1/agent.proto` — agent service definition
- `shared/proto/sober/scheduler/v1/scheduler.proto` — scheduler service definition

---

## Gitignore and Dockerignore

`.gitignore` covers:
- `target/` (Rust build artifacts)
- `node_modules/`
- `.env` (secrets)
- `build/` (frontend production build)
- `.svelte-kit/` (SvelteKit generated files)

`.dockerignore` covers:
- `target/`
- `node_modules/`
- `.git/`
