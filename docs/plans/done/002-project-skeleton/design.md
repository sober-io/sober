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
| `sober-web` | binary | sober-core |

Binary crates have `main.rs`; library crates have `lib.rs`. `sober-cli` produces
two binaries (`sober` and `soberctl`) via `[[bin]]` sections.

Dependency flow is strictly downward: `agent -> {mind, mcp, sandbox, llm, memory} -> core`
and `api -> auth -> crypto -> core`. `sober-api` and `sober-scheduler` communicate
with `sober-agent` via gRPC/UDS at runtime, not as crate dependencies. Proto
definitions live in `backend/proto/`.

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

## CI/CD Pipelines

Three GitHub Actions workflows, set up from day one.

### `ci.yml` — Lint, Test, Check

**Triggers:** PR opened/updated, push to main

| Job | Steps |
|-----|-------|
| `rust-check` | `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace`, `cargo audit` |
| `frontend-check` | `pnpm install`, `pnpm check` (svelte-check + tsc), `pnpm lint` (prettier + eslint) |

Runs on `ubuntu-latest`. Fast feedback, no artifacts produced. Uses caching for
`~/.cargo/registry` and `target/`.

### `release.yml` — Binary Builds

**Triggers:** push tag `v*` (e.g., `v0.1.0`)

Build matrix (multi-arch):

| Target | Runner |
|--------|--------|
| `x86_64-unknown-linux-gnu` | `ubuntu-latest` |
| `aarch64-unknown-linux-gnu` | `ubuntu-latest` + `cross` |
| `x86_64-apple-darwin` | `macos-latest` |
| `aarch64-apple-darwin` | `macos-latest` |

Produces per-platform tar.gz archives containing all binaries: `sober`, `soberctl`,
`sober-api`, `sober-agent`, `sober-scheduler`, `sober-web`. Uploaded as GitHub
Release assets.

### `docker.yml` — Container Images

**Triggers:** push to main (`:latest` tag), push tag `v*` (`:v0.1.0` tag)

Four service images, multi-arch (`linux/amd64`, `linux/arm64`):

| Image | Binary | Purpose |
|-------|--------|---------|
| `ghcr.io/.../sober-api` | `sober-api` | API server (headless) |
| `ghcr.io/.../sober-agent` | `sober-agent` | Agent gRPC server |
| `ghcr.io/.../sober-scheduler` | `sober-scheduler` | Tick engine |
| `ghcr.io/.../sober-web` | `sober-web` | Static assets + API reverse proxy |

Multi-stage Dockerfile:

```
Stage 1: rust:latest          — cargo build --release
Stage 2: debian:trixie-slim   — copy binary, ca-certificates, create sober user
```

`sober-web` has an additional first stage:

```
Stage 0: node:24-slim          — pnpm install && pnpm build (static output)
Stage 1: rust:latest           — cargo build --release -p sober-web
Stage 2: debian:trixie-slim    — copy binary + copy static assets
```

### Caching

All workflows use:
- `actions/cache` for `~/.cargo/registry` and `target/`
- Docker layer caching via `docker/build-push-action` cache-to/cache-from

### Registry

GHCR (GitHub Container Registry). Free for private repos with GitHub Actions.
Seamless transition when repo goes public.

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

## Proto Definitions

`backend/proto/` holds gRPC service definitions (Protocol Buffers) for internal
inter-process communication. `sober-api` and `sober-scheduler` use these proto
files to generate gRPC client stubs for calling `sober-agent`. `sober-agent`
uses them to generate its gRPC server implementation.

Proto file layout:
- `backend/proto/sober/agent/v1/agent.proto` — agent service definition
- `backend/proto/sober/scheduler/v1/scheduler.proto` — scheduler service definition

---

## sober-web Crate

`sober-web` is the single public-facing HTTP entry point. It serves the SvelteKit
frontend and reverse-proxies API/WebSocket traffic to `sober-api`.

### Routing

```
Browser -> sober-web (:3000)
              |-- /api/*     -> proxy to sober-api (UDS or localhost)
              |-- /ws        -> proxy WebSocket to sober-api
              |-- /*         -> serve static SvelteKit assets (SPA fallback)
```

### Asset Serving

Two modes:

- **Embedded (default)** — static assets compiled into the binary via `rust-embed`.
  Single binary, zero external files. Best for binary distribution.
- **Directory override** — `sober-web --static-dir /path/to/assets/` serves from
  filesystem instead. Best for development and custom deployments.

### Deployment Topology

| Process | Port/Socket | Public |
|---------|-------------|--------|
| `sober-web` | `:3000` (configurable) | Yes |
| `sober-api` | UDS or `localhost:8080` | No |
| `sober-agent` | UDS | No |
| `sober-scheduler` | UDS | No |

`sober-api` can still be deployed standalone for headless/API-only use cases
(bots, programmatic access). `sober-web` is the "full stack" entry point.

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

---

## Bare-Metal Installation & Service Management

For non-Docker deployments, `scripts/install.sh` handles downloading binaries
from GitHub Releases, creating the directory structure, collecting configuration,
setting up a system user, and installing systemd services. The same script handles
fresh installs and upgrades.

### Directory Layout

```
/opt/sober/bin/              # all binaries (api, agent, scheduler, web, sober, soberctl)
/opt/sober/data/             # persistent data (default, configurable)
  ├── workspaces/            # user workspaces (git repos, project files)
  ├── blobs/                 # blob storage
  └── keys/                  # cryptographic keypairs
/etc/sober/                  # config.toml + .env (secrets)
/run/sober/                  # UDS sockets (tmpfs, cleared on reboot)
/usr/local/bin/sober         # symlink → /opt/sober/bin/sober
/usr/local/bin/soberctl      # symlink → /opt/sober/bin/soberctl
/etc/systemd/system/         # sober-*.service + sober.target
```

All paths under `/opt/sober/data/` are defaults, individually overridable in
`config.toml`.

### Configuration

Two files in `/etc/sober/`:

**`config.toml`** — structural configuration:

```toml
[server]
host = "0.0.0.0"
port = 3000

[storage]
data_dir = "/opt/sober/data"
# workspaces_dir = "/mnt/big-disk/sober/workspaces"
# blobs_dir = "/mnt/big-disk/sober/blobs"

[database]
max_connections = 10

[qdrant]
url = "http://localhost:6334"

[logging]
level = "info"
```

**`.env`** — secrets only, mode `0600`:

```bash
DATABASE_URL=postgres://sober:password@localhost/sober
LLM_BASE_URL=https://openrouter.ai/api/v1
LLM_API_KEY=sk-...
LLM_MODEL=anthropic/claude-sonnet-4
```

Services load `config.toml` for structure and `.env` for secrets. The `.env` is
referenced via `EnvironmentFile=` in systemd units.

### User Management

```bash
# Default: creates `sober` system user
./install.sh

# Use existing user
./install.sh --user=myuser

# Non-interactive (automation/Ansible)
./install.sh --user=sober --yes
```

- Default `sober` user created with
  `useradd --system --no-create-home --home-dir /opt/sober/data --shell /usr/sbin/nologin sober`
- `/opt/sober/` owned by `sober:sober`
- `/etc/sober/.env` owned by `sober:sober`, mode `0600`
- `/run/sober/` created via `tmpfiles.d` or systemd `RuntimeDirectory=`
- Using `--user=root` prints a warning but is allowed
- No dedicated `--root` flag

### Systemd Services

Individual unit files + a target for group management.

**`sober.target`** — brings up everything:

```ini
[Unit]
Description=Sõber AI Agent System
After=network-online.target
Wants=sober-agent.service sober-api.service sober-scheduler.service sober-web.service

[Install]
WantedBy=multi-user.target
```

**Service dependency chain:**

```
sober-agent.service          (starts first — no deps on other sober services)
  ├── sober-api.service      (After=sober-agent.service)
  ├── sober-scheduler.service (After=sober-agent.service)
  └── sober-web.service      (After=sober-api.service)
```

**Example unit (`sober-agent.service`):**

```ini
[Unit]
Description=Sõber Agent (gRPC)
After=network-online.target
PartOf=sober.target

[Service]
Type=notify
User=sober
Group=sober
ExecStart=/opt/sober/bin/sober-agent
EnvironmentFile=/etc/sober/.env
Environment=SOBER_CONFIG=/etc/sober/config.toml
RuntimeDirectory=sober
StateDirectory=sober
Restart=on-failure
RestartSec=5

# Hardening
ProtectSystem=strict
ProtectHome=yes
NoNewPrivileges=yes
PrivateTmp=yes
ReadWritePaths=/opt/sober/data

[Install]
WantedBy=sober.target
```

Usage:

```bash
systemctl enable --now sober.target    # start everything + enable on boot
systemctl restart sober-agent          # restart one service
systemctl status sober-web             # check individual status
journalctl -u sober-api -f             # follow logs
```

### Install Script

```bash
./install.sh [OPTIONS]

Options:
  --user=<name>        Run services as this user (default: sober, creates if missing)
  --version=<tag>      Install specific version (default: latest)
  --yes                Non-interactive mode, skip confirmation prompts
  --uninstall          Remove binaries and services, preserve config and data
  --database-url=...   Set DATABASE_URL (skip prompt)
  --llm-base-url=...   Set LLM_BASE_URL (skip prompt)
  --llm-api-key=...    Set LLM_API_KEY (skip prompt)
  --llm-model=...      Set LLM_MODEL (skip prompt)
```

**Fresh install flow:**

1. Detect OS and architecture (`x86_64`/`aarch64`, Linux only)
2. Check prerequisites: `systemctl`, `curl`/`wget`
3. Create system user (if default or `--user` doesn't exist)
4. Create directory structure (`/opt/sober/`, `/etc/sober/`, etc.)
5. Download binary archive from GitHub Releases, verify checksum
6. Extract binaries to `/opt/sober/bin/`, create symlinks in `/usr/local/bin/`
7. Prompt for required config values (or accept via flags):
   `DATABASE_URL`, `LLM_BASE_URL`, `LLM_API_KEY`, `LLM_MODEL`
8. Validate database connection
9. Write `config.toml` and `.env`
10. Install systemd unit files and `sober.target`
11. `systemctl daemon-reload && systemctl enable --now sober.target`
12. Health check — verify services started

**Upgrade flow (existing install detected):**

1. Detect current version from `/opt/sober/bin/sober-api --version`
2. Download new binaries
3. Stop services: `systemctl stop sober.target`
4. Replace binaries in `/opt/sober/bin/`
5. Restart services: `systemctl start sober.target`
6. Health check
7. Preserve existing config — no prompts for env values

**Uninstall flow (`--uninstall`):**

1. Stop and disable services: `systemctl stop sober.target && systemctl disable sober.target`
2. Remove systemd unit files and target
3. `systemctl daemon-reload`
4. Remove binaries: `/opt/sober/bin/`
5. Remove symlinks: `/usr/local/bin/sober`, `/usr/local/bin/soberctl`
6. Print remaining paths:
   - `Configuration preserved at /etc/sober/`
   - `Data preserved at /opt/sober/data/`
   - `To remove manually: rm -rf /etc/sober /opt/sober/data && userdel sober`
