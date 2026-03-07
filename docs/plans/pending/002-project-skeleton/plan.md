# 002 — Project Skeleton: Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Bootstrap the Sober repository from docs-only into a buildable, testable
project skeleton with CI/CD pipelines and all crate stubs.

**Architecture:** Cargo workspace with 13 crates (8 library, 5 binary), SvelteKit
frontend, three GitHub Actions workflows (CI, release, Docker), `sober-web` as
the public-facing entry point, and bare-metal installation with systemd service
management.

**Tech Stack:** Rust 2024 edition, SvelteKit + Tailwind CSS, GitHub Actions, GHCR,
Docker multi-stage builds, `debian:trixie-slim` base image.

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

Create all thirteen crates under `backend/crates/`:

**Library crates** (each has `Cargo.toml` + `src/lib.rs`):
- `sober-core` — no internal dependencies
- `sober-crypto` — depends on sober-core
- `sober-auth` — depends on sober-crypto, sober-core
- `sober-memory` — depends on sober-core
- `sober-llm` — depends on sober-core
- `sober-sandbox` — depends on sober-core; external dep: `bwrap` process sandboxing
- `sober-mcp` — depends on sober-sandbox, sober-core
- `sober-mind` — depends on sober-memory, sober-crypto, sober-core

**Binary crates** (each has `Cargo.toml` + `src/main.rs`):
- `sober-agent` — `[[bin]] name = "sober-agent"`, gRPC server (tonic), depends on sober-mind, sober-mcp, sober-sandbox, sober-llm, sober-memory, sober-core
- `sober-api` — `[[bin]] name = "sober-api"`, depends on sober-auth, sober-core
- `sober-scheduler` — `[[bin]] name = "sober-scheduler"`, depends on sober-core, sober-crypto
- `sober-cli` — two `[[bin]]` sections (`sober` and `soberctl`), depends on sober-crypto, sober-core
- `sober-web` — `[[bin]] name = "sober-web"`, depends on sober-core; external deps: `rust-embed`, `axum`, `hyper-util` (for reverse proxy)

Each `Cargo.toml` inherits `edition.workspace = true` and uses `dep.workspace = true`
for shared dependencies where applicable.

Library `lib.rs` files contain a doc comment describing the crate's purpose.
Binary `main.rs` files contain a minimal `fn main()` with a placeholder print.

- [ ] All thirteen `backend/crates/*/Cargo.toml` files exist
- [ ] All thirteen `backend/crates/*/src/{lib,main}.rs` files exist
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

Define commands at the project root. All commands use quiet flags (`-q`,
`--silent`) to suppress verbose output and only print errors — this saves
tokens in interactive AI tool sessions:

- `dev` — start backend (cargo-watch) and frontend (pnpm dev) concurrently
- `build` — `cargo build -q --release` + `pnpm build --silent`
- `test` — `cargo test --workspace -q` + `pnpm test --silent`
- `check` — `cargo check -q` + `cargo clippy -q -- -D warnings` + `pnpm check`
- `fmt` — `cargo fmt --check -q` + `pnpm format`
- `lint` — `cargo clippy -q -- -D warnings` + `pnpm lint`
- `audit` — `cargo audit -q`

- [ ] File exists at project root
- [ ] All commands are defined (syntactically valid)

### 7. Create CI workflow

Create `.github/workflows/ci.yml`:
- Triggers on push to main and pull_request
- Two jobs:
  - `rust-check`: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace`, `cargo audit`
  - `frontend-check`: `pnpm install`, `pnpm check`, `pnpm lint`
- Caches `~/.cargo/registry` and `target/` via `actions/cache`
- Uses latest stable Rust and Node.js 24
- Runs on `ubuntu-latest`

- [ ] File exists at `.github/workflows/ci.yml`
- [ ] YAML is valid (`yq` or manual inspection)

### 7b. Create release workflow

Create `.github/workflows/release.yml`:
- Triggers on push tag `v*`
- Build matrix for multi-arch binaries:
  - `x86_64-unknown-linux-gnu` on `ubuntu-latest`
  - `aarch64-unknown-linux-gnu` on `ubuntu-latest` using `cross`
  - `x86_64-apple-darwin` on `macos-latest`
  - `aarch64-apple-darwin` on `macos-latest`
- Builds all binaries: `sober`, `soberctl`, `sober-api`, `sober-agent`, `sober-scheduler`, `sober-web`
- After building, assembles a staging directory per platform:
  ```bash
  mkdir -p staging/{bin,systemd,config}
  cp target/$TARGET/release/{sober,soberctl,sober-api,sober-agent,sober-scheduler,sober-web} staging/bin/
  cp infra/systemd/* staging/systemd/
  cp infra/config/* staging/config/
  tar -czf "sober-${TAG}-${TARGET}.tar.gz" -C staging .
  sha256sum "sober-${TAG}-${TARGET}.tar.gz" > "sober-${TAG}-${TARGET}.tar.gz.sha256"
  ```
- Creates GitHub Release and uploads archives + checksum files as assets
- Caches `~/.cargo/registry` and `target/`

- [ ] File exists at `.github/workflows/release.yml`
- [ ] YAML is valid
- [ ] Archive contains `bin/`, `systemd/`, and `config/` subdirectories
- [ ] `.sha256` checksum file uploaded alongside each archive

### 7c. Create Docker workflow

Create `.github/workflows/docker.yml`:
- Triggers on push to main (`:latest` tag) and push tag `v*` (`:v0.1.0` tag)
- Builds four service images, multi-arch (`linux/amd64`, `linux/arm64`):
  - `ghcr.io/${{ github.repository }}/sober-api`
  - `ghcr.io/${{ github.repository }}/sober-agent`
  - `ghcr.io/${{ github.repository }}/sober-scheduler`
  - `ghcr.io/${{ github.repository }}/sober-web`
- Uses `docker/build-push-action` with `docker/setup-buildx-action` and `docker/setup-qemu-action`
- Logs in to GHCR via `docker/login-action` using `GITHUB_TOKEN`
- Uses Docker layer caching via `cache-to`/`cache-from` (GitHub Actions cache backend)

Create Dockerfiles:
- `infra/docker/Dockerfile.service` — shared multi-stage Dockerfile for API, agent, scheduler:
  - Stage 1: `rust:latest` — `cargo build --release -p $SERVICE`
  - Stage 2: `debian:trixie-slim` — copy binary, install `ca-certificates`, create `sober` user
  - Uses `ARG SERVICE` to parameterize which binary to build/copy
- `infra/docker/Dockerfile.web` — multi-stage Dockerfile for sober-web:
  - Stage 0: `node:24-slim` — `pnpm install && pnpm build` (SvelteKit static output)
  - Stage 1: `rust:latest` — `cargo build --release -p sober-web`
  - Stage 2: `debian:trixie-slim` — copy binary + copy static assets to `/opt/sober/data/static/`

- [ ] Workflow file exists at `.github/workflows/docker.yml`
- [ ] `infra/docker/Dockerfile.service` exists and is valid
- [ ] `infra/docker/Dockerfile.web` exists and is valid
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
- **Do NOT ignore** `backend/.sqlx/` — this is the offline query cache and must be committed

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

- `test_db()` — creates a test PostgreSQL connection pool using `DATABASE_URL` from env,
  runs migrations, returns `PgPool`. Wraps each test in a transaction that rolls back.
- `test_config()` — returns an `AppConfig` populated with test defaults.
- `MockLlmEngine` — **deferred to plan 008** (sober-llm) when the `LlmEngine` trait exists.
  Stub module with a TODO comment only in this plan.
- `MockGrpcServer` — **deferred to plan 012** (sober-agent) when the gRPC service is defined.
  Stub module with a TODO comment only in this plan.

Each downstream crate can depend on `sober-core` with `features = ["test-utils"]` in
`[dev-dependencies]`.

For bwrap-dependent tests (plan 009): gate behind `#[cfg(target_os = "linux")]` and skip
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

### 13. Create systemd service units

Create systemd unit files in `infra/systemd/`:

- `infra/systemd/sober.target` — group target that brings up all services:
  ```ini
  [Unit]
  Description=Sõber AI Agent System
  After=network-online.target
  Wants=sober-agent.service sober-api.service sober-scheduler.service sober-web.service

  [Install]
  WantedBy=multi-user.target
  ```

- `infra/systemd/sober-agent.service` — gRPC agent server:
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
  ProtectSystem=strict
  ProtectHome=yes
  NoNewPrivileges=yes
  PrivateTmp=yes
  ReadWritePaths=/opt/sober/data

  [Install]
  WantedBy=sober.target
  ```

- `infra/systemd/sober-api.service` — same structure, `After=network-online.target sober-agent.service`,
  `ExecStart=/opt/sober/bin/sober-api`

- `infra/systemd/sober-scheduler.service` — same structure, `After=network-online.target sober-agent.service`,
  `ExecStart=/opt/sober/bin/sober-scheduler`

- `infra/systemd/sober-web.service` — same structure, `After=network-online.target sober-api.service`,
  `ExecStart=/opt/sober/bin/sober-web`

All services share the same hardening directives. Each has the correct `After=`
dependency per the design.

- [ ] All five files exist in `infra/systemd/`
- [ ] `systemd-analyze verify infra/systemd/*.service` passes (if systemd is available)

### 14. Create default `config.toml` template

Create `infra/config/config.toml.example` — the default config template that
`install.sh` copies to `/etc/sober/config.toml`:

```toml
[server]
host = "0.0.0.0"
port = 3000

[storage]
data_dir = "/opt/sober/data"
# Override individual subdirectories:
# workspaces_dir = "/mnt/big-disk/sober/workspaces"
# blobs_dir = "/mnt/big-disk/sober/blobs"

[database]
max_connections = 10

[qdrant]
url = "http://localhost:6334"

[logging]
level = "info"
```

- [ ] File exists at `infra/config/config.toml.example`
- [ ] TOML is valid

### 15. Create `install.sh`

Create `install.sh` at the project root. The script handles fresh installs,
upgrades, and uninstalls. Must be POSIX-compatible shell (no bashisms beyond
what's needed for readability — target `#!/usr/bin/env bash` with `set -euo pipefail`).

**Variables and defaults:**

```bash
SOBER_USER="${SOBER_USER:-sober}"
SOBER_VERSION="${SOBER_VERSION:-latest}"
INSTALL_DIR="/opt/sober"
CONFIG_DIR="/etc/sober"
SYSTEMD_DIR="/etc/systemd/system"
GITHUB_REPO="<owner>/sober"  # replace with actual repo
```

**Step 1: Argument parsing**

Parse CLI flags:
- `--user=<name>` — set `SOBER_USER`
- `--version=<tag>` — set `SOBER_VERSION`
- `--yes` — set `NONINTERACTIVE=1`
- `--uninstall` — set `UNINSTALL=1`
- `--database-url=<url>` — set `DATABASE_URL`
- `--llm-base-url=<url>` — set `LLM_BASE_URL`
- `--llm-api-key=<key>` — set `LLM_API_KEY`
- `--llm-model=<model>` — set `LLM_MODEL`

**Step 2: Detect mode**

```bash
detect_mode() {
    if [ "${UNINSTALL:-0}" = "1" ]; then
        echo "uninstall"
    elif [ -x "$INSTALL_DIR/bin/sober-api" ]; then
        echo "upgrade"
    else
        echo "install"
    fi
}
```

**Step 3: Prerequisite checks**

```bash
check_prerequisites() {
    command -v systemctl >/dev/null 2>&1 || die "systemctl not found — systemd is required"
    command -v curl >/dev/null 2>&1 || command -v wget >/dev/null 2>&1 || die "curl or wget required"

    ARCH=$(uname -m)
    case "$ARCH" in
        x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
        aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
        *)       die "Unsupported architecture: $ARCH" ;;
    esac

    [ "$(uname -s)" = "Linux" ] || die "Only Linux is supported for bare-metal install"
    [ "$(id -u)" -eq 0 ] || die "install.sh must be run as root"
}
```

**Step 4: User creation**

```bash
ensure_user() {
    if [ "$SOBER_USER" = "root" ]; then
        warn "Running as root is not recommended. Consider a dedicated service user."
        return
    fi

    if id "$SOBER_USER" >/dev/null 2>&1; then
        info "User '$SOBER_USER' already exists"
    else
        info "Creating system user '$SOBER_USER'"
        useradd --system --no-create-home \
            --home-dir "$INSTALL_DIR/data" \
            --shell /usr/sbin/nologin \
            "$SOBER_USER"
    fi
}
```

**Step 5: Directory creation**

```bash
create_directories() {
    mkdir -p "$INSTALL_DIR/bin"
    mkdir -p "$INSTALL_DIR/data/workspaces"
    mkdir -p "$INSTALL_DIR/data/blobs"
    mkdir -p "$INSTALL_DIR/data/keys"
    mkdir -p "$CONFIG_DIR"

    chown -R "$SOBER_USER:$SOBER_USER" "$INSTALL_DIR"
    chown -R "$SOBER_USER:$SOBER_USER" "$CONFIG_DIR"
}
```

**Step 6: Download and extract release archive**

The release archive has subdirectories:

```
sober-<version>-<target>.tar.gz
├── bin/           # binaries
├── systemd/       # unit files + target
└── config/        # config.toml.example
```

```bash
download_and_extract() {
    if [ "$SOBER_VERSION" = "latest" ]; then
        SOBER_VERSION=$(fetch "https://api.github.com/repos/$GITHUB_REPO/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
    fi

    local archive="sober-${SOBER_VERSION}-${TARGET}.tar.gz"
    local url="https://github.com/$GITHUB_REPO/releases/download/${SOBER_VERSION}/${archive}"
    local checksum_url="${url}.sha256"

    info "Downloading Sõber $SOBER_VERSION for $TARGET"
    fetch "$url" -o "/tmp/$archive"
    fetch "$checksum_url" -o "/tmp/${archive}.sha256"

    info "Verifying checksum"
    (cd /tmp && sha256sum -c "${archive}.sha256") || die "Checksum verification failed"

    EXTRACT_DIR=$(mktemp -d)
    tar -xzf "/tmp/$archive" -C "$EXTRACT_DIR"
    rm -f "/tmp/$archive" "/tmp/${archive}.sha256"

    info "Installing binaries to $INSTALL_DIR/bin/"
    cp "$EXTRACT_DIR/bin/"* "$INSTALL_DIR/bin/"
    chmod +x "$INSTALL_DIR/bin/"*

    # Symlinks for CLI tools
    ln -sf "$INSTALL_DIR/bin/sober" /usr/local/bin/sober
    ln -sf "$INSTALL_DIR/bin/soberctl" /usr/local/bin/soberctl
}
```

Use a `fetch()` helper that wraps `curl -fsSL` or `wget -qO-` depending on
availability. `$EXTRACT_DIR` is reused by steps 7 and 8, then cleaned up in
the main flow.

**Step 7: Collect configuration (fresh install only)**

```bash
collect_config() {
    # Skip if config already exists (upgrade)
    [ -f "$CONFIG_DIR/.env" ] && return

    prompt_required "DATABASE_URL" "PostgreSQL connection string" "postgres://sober:password@localhost/sober"
    prompt_required "LLM_BASE_URL" "LLM API base URL" ""
    prompt_required "LLM_API_KEY" "LLM API key" ""
    prompt_required "LLM_MODEL" "LLM model identifier" ""

    validate_database "$DATABASE_URL"

    # Write .env
    cat > "$CONFIG_DIR/.env" <<EOF
DATABASE_URL=$DATABASE_URL
LLM_BASE_URL=$LLM_BASE_URL
LLM_API_KEY=$LLM_API_KEY
LLM_MODEL=$LLM_MODEL
EOF
    chmod 0600 "$CONFIG_DIR/.env"
    chown "$SOBER_USER:$SOBER_USER" "$CONFIG_DIR/.env"

    # Write config.toml from bundled template
    cp "$EXTRACT_DIR/config/config.toml.example" "$CONFIG_DIR/config.toml" 2>/dev/null \
        || write_default_config
    chown "$SOBER_USER:$SOBER_USER" "$CONFIG_DIR/config.toml"
}
```

`prompt_required()` checks if the variable was set via CLI flag first, then
prompts interactively (unless `--yes` mode, where missing required values are
a fatal error). `validate_database()` attempts a connection using `psql` or
`pg_isready` if available, with a warning (not fatal) if neither tool is found.

**Step 8: Install systemd units**

```bash
install_systemd() {
    local services="sober-agent sober-api sober-scheduler sober-web"

    for svc in $services; do
        sed "s/User=sober/User=$SOBER_USER/g; s/Group=sober/Group=$SOBER_USER/g" \
            "$EXTRACT_DIR/systemd/${svc}.service" > "$SYSTEMD_DIR/${svc}.service"
    done

    cp "$EXTRACT_DIR/systemd/sober.target" "$SYSTEMD_DIR/sober.target"

    systemctl daemon-reload
    systemctl enable sober.target
}
```

The systemd unit files are bundled in the `systemd/` subdirectory of the
release archive. The script patches the `User=`/`Group=` lines to match `--user`.

**Step 9: Run database migrations**

```bash
run_migrations() {
    info "Running database migrations"
    sudo -u "$SOBER_USER" "$INSTALL_DIR/bin/sober" migrate run \
        || die "Migration failed. Check DATABASE_URL in $CONFIG_DIR/.env"
    info "Migrations complete"
}
```

Migrations are embedded in the `sober` binary via `sqlx::migrate!()` — no SQL
files on disk. This step runs on both fresh install and upgrade. sqlx tracks
which migrations have already been applied, so re-running is safe.

**Step 10: Start services and health check**

```bash
start_and_verify() {
    info "Starting Sõber services"
    systemctl start sober.target

    sleep 3

    local failed=0
    for svc in sober-agent sober-api sober-scheduler sober-web; do
        if systemctl is-active --quiet "$svc"; then
            info "$svc: running"
        else
            warn "$svc: failed to start (check: journalctl -u $svc)"
            failed=1
        fi
    done

    if [ "$failed" = "0" ]; then
        info "Sõber is running. Access the web UI at http://localhost:3000"
    else
        warn "Some services failed to start. Check logs with: journalctl -u sober-*"
    fi
}
```

**Step 10: Upgrade flow**

```bash
do_upgrade() {
    local current_version
    current_version=$("$INSTALL_DIR/bin/sober-api" --version 2>/dev/null | awk '{print $2}')
    info "Current version: ${current_version:-unknown}"

    download_and_extract

    info "Stopping services"
    systemctl stop sober.target

    info "Binaries updated. Running migrations and restarting"
    install_systemd
    run_migrations
    start_and_verify
}
```

**Step 11: Uninstall flow**

```bash
do_uninstall() {
    info "Stopping and disabling Sõber services"
    systemctl stop sober.target 2>/dev/null || true
    systemctl disable sober.target 2>/dev/null || true

    rm -f "$SYSTEMD_DIR"/sober-*.service "$SYSTEMD_DIR/sober.target"
    systemctl daemon-reload

    rm -rf "$INSTALL_DIR/bin"
    rm -f /usr/local/bin/sober /usr/local/bin/soberctl

    info "Sõber binaries and services removed."
    info ""
    info "The following data was preserved:"
    [ -d "$CONFIG_DIR" ] && info "  Configuration: $CONFIG_DIR/"
    [ -d "$INSTALL_DIR/data" ] && info "  Data:          $INSTALL_DIR/data/"
    info ""
    info "To remove manually:"
    info "  rm -rf $CONFIG_DIR $INSTALL_DIR/data"
    [ "$SOBER_USER" != "root" ] && info "  userdel $SOBER_USER"
}
```

**Main entry point:**

```bash
cleanup() {
    [ -n "${EXTRACT_DIR:-}" ] && rm -rf "$EXTRACT_DIR"
}
trap cleanup EXIT

main() {
    check_prerequisites

    MODE=$(detect_mode)

    case "$MODE" in
        install)
            info "Fresh install of Sõber"
            ensure_user
            create_directories
            download_and_extract
            collect_config
            install_systemd
            run_migrations
            start_and_verify
            ;;
        upgrade)
            info "Upgrading Sõber"
            do_upgrade
            ;;
        uninstall)
            do_uninstall
            ;;
    esac
}

main "$@"
```

- [ ] `install.sh` exists at project root
- [ ] Script is executable (`chmod +x`)
- [ ] `shellcheck install.sh` passes with no errors
- [ ] `--help` flag prints usage information
- [ ] Script detects missing prerequisites and exits with clear error message

---

## Acceptance Criteria

All of the following must pass before this plan is considered complete:

- [ ] `cargo build` succeeds in `backend/`
- [ ] `cargo test --workspace` succeeds in `backend/` (compiles, zero tests is acceptable)
- [ ] `cargo clippy -- -D warnings` produces no warnings in `backend/`
- [ ] `pnpm install && pnpm check` succeeds in `frontend/`
- [ ] `docker compose config` validates without error at project root
- [ ] All `justfile` commands are defined and syntactically valid
- [ ] CI workflow YAML is valid (`.github/workflows/ci.yml`)
- [ ] Release workflow YAML is valid (`.github/workflows/release.yml`)
- [ ] Docker workflow YAML is valid (`.github/workflows/docker.yml`)
- [ ] Dockerfiles exist and are syntactically valid (`infra/docker/Dockerfile.service`, `infra/docker/Dockerfile.web`)
- [ ] All thirteen crates exist (including `sober-web`)
- [ ] No secrets are committed (no `.env` file, only `.env.example`)
- [ ] Systemd unit files exist in `infra/systemd/` (5 files)
- [ ] `config.toml.example` exists in `infra/config/`
- [ ] `install.sh` exists, is executable, and passes `shellcheck`
- [ ] Release archive includes systemd units, config template, and sha256 checksums
