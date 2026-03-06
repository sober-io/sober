# 000 — Bootstrap Gap Analysis

> Critical review of project docs before any code is written.
> Date: 2025-03-05

---

## 1. Gaps — Missing Pieces That Block Development

### G1: No code exists at all

The repo is entirely docs. No `Cargo.toml`, `package.json`, `justfile`, `.env.example`,
`Dockerfile`, or source files. Everything described in CLAUDE.md's "Build & Run" section
will fail immediately.

**Resolution:** Follow bootstrap order (Section 6) to stand up the project skeleton.

### G2: No justfile / task runner

CLAUDE_CODE_BOOTSTRAP.md references `just dev`, `just build`, `just test`, `just check`,
`just fmt` — but no justfile exists.

**Resolution:** Create justfile in the first bootstrap commit, before any crate work.

### G3: No `.env.example`

Bootstrap doc says "typed config struct populated from env vars at startup" but no
`.env.example` documents which variables are needed.

**Resolution:** Create `.env.example` with at least:
```
DATABASE_URL=postgres://...
QDRANT_URL=http://localhost:6334
REDIS_URL=redis://localhost:6379
LLM_API_KEY=
ADMIN_SOCKET_PATH=/run/sober/admin.sock
```

### G4: No Docker / infra configuration

CLAUDE.md lists `docker compose up -d` and the repo structure shows `infra/` for Docker/K8s
configs, but nothing exists. The backend needs PostgreSQL, Qdrant, and Redis at minimum.

**Resolution:** Create `docker-compose.yml` (dev profile) with postgres, qdrant, and redis
services before any crate that touches a database.

### G5: No CI/CD configuration

No GitHub Actions, no `.github/` directory. CLAUDE.md and rust-patterns.md reference
running `cargo clippy`, `cargo audit`, and `cargo-nextest` in CI but no pipeline exists.

**Resolution:** Add basic CI (lint + test + audit) after the first crate compiles. Not a
day-one blocker but should come before any PR merges.

### G6: No shared protobuf schemas

Repo structure shows `shared/` for "Protobuf schemas, shared types" but no `.proto` files
exist. No protobuf tooling is mentioned in dependencies (no `prost`, no `tonic`).

**Resolution:** Decide: are protobufs actually needed? If the API is JSON/REST (as
rust-patterns.md describes) and there's no gRPC, drop protobuf from the plan. If inter-service
communication needs it later, add it then. Remove `shared/` from the initial structure or
repurpose it for shared TypeScript/Rust types without protobuf.

### G7: No migration tooling setup

CLAUDE.md references `backend/migrations/` for sqlx migrations, but sqlx compile-time
checking requires a running database and `DATABASE_URL` set. This creates a chicken-and-egg
with Docker setup.

**Resolution:** Docker compose (G4) must come before any sqlx migration work. Use
`sqlx-cli` (`cargo install sqlx-cli`) and set up the first migration as part of sober-core
bootstrap.

---

## 2. Contradictions Between Docs

### C1: Flat backend structure vs. workspace with crates

**CLAUDE_CODE_BOOTSTRAP.md** describes:
```
backend/
  src/
    main.rs, lib.rs, config.rs, error.rs
    routes/, services/, models/, middleware/
```

**CLAUDE.md + ARCHITECTURE.md** describe:
```
backend/
  crates/
    sober-core/, sober-auth/, sober-agent/, ...
  migrations/
```

These are incompatible. A flat `src/` structure cannot coexist with a multi-crate workspace
in the way described.

**Resolution:** The workspace model (CLAUDE.md/ARCHITECTURE.md) is the real design — it
matches the crate map and dependency rules. Treat CLAUDE_CODE_BOOTSTRAP.md's structure as
a simplified starting point that was superseded. The bootstrap doc should be treated as
generic guidance only; CLAUDE.md and ARCHITECTURE.md are authoritative.

### C2: `cargo init backend` vs. workspace

Bootstrap says `cargo init backend` which creates a single binary project. The actual
design requires `cargo workspace` with 10 crates.

**Resolution:** Use `cargo new backend` then immediately convert to a workspace `Cargo.toml`
with `[workspace] members = ["crates/*"]`.

### C3: sober-cli dependency rule vs. soberctl needs

CLAUDE.md says: "`sober-cli` depends on `sober-core` only."
But `soberctl` needs to send commands over a Unix socket to the API server. It needs to
serialize/deserialize request/response types that the API understands.

**Resolution:** This works if `sober-core` defines the admin command/response types (which
makes sense — they're shared domain types). Both `sober-cli` and `sober-api` depend on
`sober-core`, so the types are shared without `sober-cli` depending on `sober-api`.
Document this explicitly: admin protocol types live in `sober-core`.

### C4: Redis mentioned in architecture but not in CLAUDE.md dependencies

ARCHITECTURE.md lists Redis for "Session tokens, rate limiting, hot context cache" in the
storage table. CLAUDE.md's Key Dependencies section has no Redis crate.

**Resolution:** Add `redis` (or `deadpool-redis`) to the dependency list when the auth/session
crate is built. Not a day-one issue but the omission should be noted.

---

## 3. Ambiguous Decisions — Mentioned But Not Decided

### A1: SvelteKit adapter — static or node?

Bootstrap says "start with static if the Rust backend serves the API" but doesn't commit.
Since the Rust backend IS the API server, static adapter is the right choice (SvelteKit
produces static files, Rust serves them + the API).

**Decision needed:** Use static adapter. The Rust API can serve the built frontend from a
`static/` directory, or they run separately in dev (vite dev server + cargo watch).

### A2: Auth starting point — passkeys or magic links?

Bootstrap says "Passkeys (WebAuthn) as the primary auth method, or magic links as a simpler
starting point." Passkeys require significantly more infrastructure (challenge generation,
credential storage, attestation verification).

**Decision needed:** Start with password + argon2id (already in the dependency list), add
passkeys as a second phase. Magic links require email infrastructure which is another
undocumented dependency.

### A3: Which LLM provider to start with?

ARCHITECTURE.md lists Anthropic, OpenAI, and Ollama. The LLM engine trait is defined but
there's no guidance on which to implement first.

**Decision needed:** Start with Anthropic (Claude) — it's the project's own ecosystem. Add
Ollama second for local dev without API costs. OpenAI third.

### A4: MCP implementation scope

`sober-mcp` is in the crate map with "MCP server/client implementation for tool interop"
but no details on protocol version, transport (stdio, SSE, HTTP), or what tools to expose.

**Decision needed:** Defer MCP to after the core agent loop works. When implemented, start
with stdio transport (simplest) and expose agent tool-calling as MCP tools.

### A5: libgit2 for code store

ARCHITECTURE.md mentions "Git (libgit2)" for "Versioned user-generated code, plugin source"
but no `git2` crate is in the dependency list, and no crate owns this responsibility.

**Decision needed:** Defer. Plugin source versioning is a later concern. When needed, add
the `gix` crate (pure Rust, actively maintained, preferred over `git2` which wraps C
libgit2).

### A6: S3/MinIO blob storage

Architecture mentions S3-compatible storage for "Large artifacts, code snapshots, binary
contexts" but no S3 crate is listed and no crate owns blob storage.

**Decision needed:** Defer. Use local filesystem for blob storage initially. Add
`aws-sdk-s3` (official AWS SDK) when needed.

---

## 4. Dependency Concerns

### D1: `aws-lc-rs` build complexity

`aws-lc-rs` requires CMake and a C compiler for building. This can cause issues on some
systems and in CI. It's the recommended replacement for `ring` but adds build-time
dependencies.

**Mitigation:** Document build prerequisites. Ensure Docker build image includes cmake.
Consider `rustls` with `aws-lc-rs` feature flag so it can be swapped if builds break.

### D2: `wasmtime` is heavyweight

`wasmtime` pulls in a large dependency tree and significantly increases compile times. For
plugin sandboxing it's the right choice, but it shouldn't be included until plugins are
actually being implemented.

**Mitigation:** Keep `sober-plugin` as a late-stage crate. Don't add `wasmtime` to
`sober-core` or any early crate.

### D3: `qdrant-client` API stability

The Qdrant Rust client has had breaking API changes between major versions. Pin to a
specific version and test upgrades carefully.

**Mitigation:** Pin exact version in `Cargo.toml`. Add integration tests that verify
Qdrant operations before upgrading.

### D4: `@anthropic-ai/sdk` on the frontend

CLAUDE.md lists `@anthropic-ai/sdk` as a frontend dependency with "(if client-side needed)".
Calling Claude directly from the browser exposes the API key to users.

**Mitigation:** Remove from frontend dependencies. All LLM calls should go through the
Rust backend. The frontend talks to the Rust API, which talks to Claude.

### D6: `bwrap` and `socat` as runtime dependencies

`sober-sandbox` requires `bubblewrap` (bwrap) and `socat` binaries at runtime for
process-level sandboxing and network proxy bridging. These are not Rust crates ---
they are system packages.

**Mitigation:** Both are widely packaged (bwrap ships with Flatpak on most distros,
socat is a standard utility). Document as prerequisites. Ensure Docker build image
includes both. `sober-sandbox` should detect missing binaries at startup and fail
with a clear error.

### D5: `openidconnect` complexity

The `openidconnect` crate has a steep learning curve and complex type signatures. It works
but requires careful setup.

**Mitigation:** Defer OIDC until password auth is solid. When implementing, start with a
single provider (Google) and wrap the complexity in a clean service interface.

---

## 5. Additional Observations

### O1: No logging/observability setup documented

Both docs mention `tracing` + `tracing-subscriber` but don't specify output format (JSON
for prod? Pretty for dev?), log levels, or whether to add OpenTelemetry.

**Resolution:** Start with `tracing-subscriber` using `fmt` layer — pretty output in dev,
JSON in prod. Add OTLP export later if needed.

### O2: No rate limiting strategy

ARCHITECTURE.md mentions rate limiting in the API gateway but no approach is documented.
Tower has `tower::limit::RateLimit` but it's per-connection, not per-user.

**Resolution:** Use `tower-governor` or a custom Redis-backed rate limiter. Decide when
building `sober-api`.

### O3: No database schema design

There's extensive architecture for memory/agents/auth but no SQL schema, no ER diagram,
and no list of tables.

**Resolution:** Create a schema design doc before writing migrations. At minimum: users,
sessions, api_keys, agents, tasks, plugins, audit_log.

---

## 6. Bootstrap Order — What Must Be Built First

The dependency graph dictates this order:

```
Phase 0: Project Skeleton
  - Workspace Cargo.toml
  - justfile
  - .env.example
  - docker-compose.yml (postgres, qdrant, redis)
  - CI pipeline (basic)

Phase 1: Foundation
  - sober-core (types, errors, config)
  - Database schema design + first migration

Phase 2: Security Layer
  - sober-crypto (keypairs, encryption, signing)

Phase 3: Storage & Auth (parallel)
  - sober-memory (vector storage, BCF basics)
  - sober-auth (password auth, sessions, RBAC)

Phase 4: Intelligence
  - sober-llm (Anthropic provider first)

Phase 5: Orchestration
  - sober-sandbox (bwrap process sandbox, policy resolution)
  - sober-agent (basic agent loop, no replicas yet)
  - sober-api (HTTP gateway, health check, auth routes)

Phase 6: CLI & Frontend (parallel)
  - sober-cli (sober + soberctl binaries)
  - frontend (SvelteKit skeleton, auth UI, chat UI)

Phase 7: Advanced Features
  - sober-plugin (registry, WASM sandbox — wasmtime or Extism TBD)
  - sober-mcp (tool interop)
  - Replica system
  - Additional auth methods (passkeys, OIDC)
```

Each phase should be a separate planning doc with acceptance criteria.

---

## Summary

| Category | Count | Blocking? |
|----------|-------|-----------|
| Gaps | 7 | G1-G4 block all development |
| Contradictions | 4 | C1-C2 must be resolved before scaffolding |
| Ambiguous decisions | 6 | A1-A2 needed for Phase 0-1 |
| Dependency concerns | 5 | D4 is a design error to fix now; rest are future |
| Observations | 3 | Non-blocking but should be addressed early |

**Immediate actions before writing any code:**
1. Resolve C1/C2: workspace structure is authoritative (done — documented above)
2. Decide A1: static adapter (recommended)
3. Decide A2: start with password auth (recommended)
4. Fix D4: remove `@anthropic-ai/sdk` from frontend deps
5. Execute Phase 0 from bootstrap order
