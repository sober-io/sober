# Docker Build Optimization — Unified Builder

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce Docker image build time from ~1 hour to ~15 minutes by compiling all Rust binaries in a single shared builder stage instead of 5 independent compilations.

**Architecture:** Replace 5 separate `cargo build --release -p <crate>` invocations (each compiling the full dependency tree independently) with one `cargo build --release` that compiles all 5 binaries at once. Each service image becomes a thin runtime stage that copies its binary from the shared builder. The Docker workflow switches from a matrix of 5 independent builds to a two-phase approach: build once, then package 5 images.

**Tech Stack:** Docker multi-stage builds, cargo-chef for dependency caching, GitHub Actions with docker/bake-action

---

## Current Problem

Each of the 5 Dockerfiles (`Dockerfile.api`, `.agent`, `.scheduler`, `.cli`, `.web`) independently:
1. Starts `FROM rust:latest`
2. Installs `protobuf-compiler`
3. Copies `backend/` source
4. Runs `cargo build --release -p <crate>`

Since all 5 crates share ~90% of the same dependency tree, this means the dependency compilation (~40 min) happens 5 times. Even with BuildKit cache mounts (each service has its own target cache ID), a Cargo.lock change invalidates all 5.

## Strategy

### Phase 1: Unified Dockerfile with cargo-chef

Replace 5 Dockerfiles with a single `Dockerfile` that uses [cargo-chef](https://github.com/LukeMathWalker/cargo-chef) for dependency caching:

1. **Chef stage** — `cargo chef prepare` generates a recipe (dependency-only manifest)
2. **Cook stage** — `cargo chef cook --release` compiles only dependencies (cached unless Cargo.lock changes)
3. **Build stage** — `cargo build --release` compiles all 5 binaries (only app code, fast)
4. **Runtime stages** — One per service, copies its binary + service-specific runtime deps

### Phase 2: Docker Bake for CI

Replace the matrix-of-5 workflow with `docker buildx bake` which builds all targets from a single `docker-bake.hcl` file, sharing the builder layers.

---

## File Structure

| File | Action | Purpose |
|------|--------|---------|
| `infra/docker/Dockerfile.ci` | Create | Unified multi-stage Dockerfile with cargo-chef (CI/prod) |
| `infra/docker/Dockerfile.api` | Keep | Dev builds — single-service fast iteration |
| `infra/docker/Dockerfile.agent` | Keep | Dev builds — single-service fast iteration |
| `infra/docker/Dockerfile.scheduler` | Keep | Dev builds — single-service fast iteration |
| `infra/docker/Dockerfile.cli` | Keep | Dev builds — single-service fast iteration |
| `infra/docker/Dockerfile.web` | Keep | Dev builds — single-service fast iteration |
| `docker-bake.hcl` | Create | Buildx bake definition for all targets |
| `docker-compose.yml` | Keep as-is | Dev compose keeps using per-service Dockerfiles |
| `.github/workflows/docker.yml` | Modify | Use bake instead of matrix build |

> **Note:** Per-service Dockerfiles stay for local development (`docker compose up --build`). The unified Dockerfile is for CI/prod image publishing only. `docker-compose.yml` is unchanged.

---

### Task 1: Create Unified Dockerfile

**Files:**
- Create: `infra/docker/Dockerfile.ci`

- [ ] **Step 1: Write the unified Dockerfile**

```dockerfile
# ============================================================
# Stage 1: Frontend build (only needed for sober-web target)
# ============================================================
FROM node:24-slim AS frontend-builder
RUN corepack enable
WORKDIR /build/frontend
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm build

# ============================================================
# Stage 2: Cargo chef — prepare dependency recipe
# ============================================================
FROM rust:latest AS chef
RUN cargo install cargo-chef --locked
RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build

# ============================================================
# Stage 3: Prepare recipe (dependency manifest only)
# ============================================================
FROM chef AS planner
COPY backend/ backend/
RUN cd backend && cargo chef prepare --recipe-path /recipe.json

# ============================================================
# Stage 4: Cook — compile dependencies only (cached layer)
# ============================================================
FROM chef AS cook
COPY --from=planner /recipe.json /recipe.json
RUN cd /build && cargo chef cook --release --recipe-path /recipe.json \
    --manifest-path backend/Cargo.toml

# ============================================================
# Stage 5: Build all binaries
# ============================================================
FROM cook AS builder
COPY backend/ backend/
# sober-web needs the frontend build output for rust-embed
COPY --from=frontend-builder /build/frontend/build/ frontend/build/
RUN cd backend && cargo build --release \
    && cp target/release/sober-api /usr/local/bin/sober-api \
    && cp target/release/sober-agent /usr/local/bin/sober-agent \
    && cp target/release/sober-scheduler /usr/local/bin/sober-scheduler \
    && cp target/release/sober-web /usr/local/bin/sober-web \
    && cp target/release/sober /usr/local/bin/sober

# ============================================================
# Runtime: sober-api
# ============================================================
FROM debian:trixie-slim AS sober-api
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --no-create-home --shell /usr/sbin/nologin sober
COPY --from=builder /usr/local/bin/sober-api /usr/local/bin/sober-api
RUN mkdir -p /run/sober && chown sober:sober /run/sober
USER sober
ENTRYPOINT ["sober-api"]

# ============================================================
# Runtime: sober-agent
# ============================================================
FROM debian:trixie-slim AS sober-agent
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates bubblewrap iputils-ping curl git \
    clang lld \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/usr/local/rustup
ENV PATH="/usr/local/cargo/bin:${PATH}"
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | CARGO_HOME=/usr/local/cargo sh -s -- -y --default-toolchain stable --profile minimal \
    && rustup target add wasm32-wasip1 \
    && chmod -R a+rX /usr/local/rustup /usr/local/cargo
RUN useradd --system --create-home --shell /usr/sbin/nologin sober
COPY --from=builder /usr/local/bin/sober-agent /usr/local/bin/sober-agent
RUN mkdir -p /run/sober /var/lib/sober/workspaces/default /home/sober/.cargo \
    && chown -R sober:sober /run/sober /var/lib/sober /home/sober
USER sober
ENV CARGO_HOME=/home/sober/.cargo
ENTRYPOINT ["sober-agent"]

# ============================================================
# Runtime: sober-scheduler
# ============================================================
FROM debian:trixie-slim AS sober-scheduler
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --no-create-home --shell /usr/sbin/nologin sober
COPY --from=builder /usr/local/bin/sober-scheduler /usr/local/bin/sober-scheduler
RUN mkdir -p /run/sober && chown sober:sober /run/sober
USER sober
ENTRYPOINT ["sober-scheduler"]

# ============================================================
# Runtime: sober-web
# ============================================================
FROM debian:trixie-slim AS sober-web
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --no-create-home --shell /usr/sbin/nologin sober
COPY --from=builder /usr/local/bin/sober-web /usr/local/bin/sober-web
COPY --from=frontend-builder /build/frontend/build/ /opt/sober/data/static/
USER sober
ENTRYPOINT ["sober-web"]

# ============================================================
# Runtime: sober-cli (also used for migrations)
# ============================================================
FROM debian:trixie-slim AS sober-cli
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --no-create-home --shell /usr/sbin/nologin sober
COPY --from=builder /usr/local/bin/sober /usr/local/bin/sober
RUN mkdir -p /run/sober && chown sober:sober /run/sober
USER sober
ENTRYPOINT ["sober"]
```

- [ ] **Step 2: Verify local build for one target**

```bash
docker build --target sober-api -t sober-api:test -f infra/docker/Dockerfile.ci .
docker run --rm sober-api:test --help
```

Expected: Binary runs and shows help output.

- [ ] **Step 3: Verify all 5 targets build from shared builder**

```bash
for target in sober-api sober-agent sober-scheduler sober-web sober-cli; do
  docker build --target $target -t $target:test -f infra/docker/Dockerfile.ci . 2>&1 | tail -1
done
```

Expected: All 5 build successfully. The builder stage is built once and cached — subsequent targets reuse it.

- [ ] **Step 4: Commit**

```bash
git add infra/docker/Dockerfile.ci
git commit -m "feat(docker): add unified multi-stage Dockerfile with cargo-chef"
```

---

### Task 2: Create docker-bake.hcl

**Files:**
- Create: `docker-bake.hcl`

- [ ] **Step 1: Write the bake definition**

```hcl
variable "REGISTRY" {
  default = "ghcr.io/sober-io/sober"
}

variable "TAG" {
  default = "latest"
}

group "default" {
  targets = ["sober-api", "sober-agent", "sober-scheduler", "sober-web", "sober-cli"]
}

target "_common" {
  dockerfile = "infra/docker/Dockerfile.ci"
  context    = "."
  platforms  = ["linux/amd64", "linux/arm64"]
  cache-from = ["type=gha"]
  cache-to   = ["type=gha,mode=max"]
}

target "sober-api" {
  inherits = ["_common"]
  target   = "sober-api"
  tags     = ["${REGISTRY}/sober-api:${TAG}"]
}

target "sober-agent" {
  inherits = ["_common"]
  target   = "sober-agent"
  tags     = ["${REGISTRY}/sober-agent:${TAG}"]
}

target "sober-scheduler" {
  inherits = ["_common"]
  target   = "sober-scheduler"
  tags     = ["${REGISTRY}/sober-scheduler:${TAG}"]
}

target "sober-web" {
  inherits = ["_common"]
  target   = "sober-web"
  tags     = ["${REGISTRY}/sober-web:${TAG}"]
}

target "sober-cli" {
  inherits = ["_common"]
  target   = "sober-cli"
  tags     = ["${REGISTRY}/sober-cli:${TAG}"]
}
```

- [ ] **Step 2: Test bake locally (dry-run)**

```bash
docker buildx bake --print
```

Expected: Prints the resolved build plan showing all 5 targets sharing the same Dockerfile.

- [ ] **Step 3: Commit**

```bash
git add docker-bake.hcl
git commit -m "feat(docker): add docker-bake.hcl for unified multi-target builds"
```

---

### Task 3: Update CI Docker Workflow

**Files:**
- Modify: `.github/workflows/docker.yml`

- [ ] **Step 1: Replace the matrix build with docker/bake-action**

```yaml
name: Docker

on:
  push:
    tags: ["v*"]
  workflow_dispatch:

permissions:
  contents: read
  packages: write

env:
  REGISTRY: ghcr.io
  FORCE_JAVASCRIPT_ACTIONS_TO_NODE24: true

jobs:
  build-and-push:
    name: Build & Push All Images
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5

      - uses: docker/setup-qemu-action@v3

      - uses: docker/setup-buildx-action@v3

      - name: Log in to GHCR
        uses: docker/login-action@v3
        with:
          registry: ${{ env.REGISTRY }}
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Resolve tag
        id: tag
        run: |
          if [[ "${{ github.ref_type }}" == "tag" ]]; then
            echo "tag=${GITHUB_REF_NAME}" >> "$GITHUB_OUTPUT"
          else
            echo "tag=latest" >> "$GITHUB_OUTPUT"
          fi

      - name: Build and push all images
        uses: docker/bake-action@v6
        with:
          files: docker-bake.hcl
          push: true
        env:
          TAG: ${{ steps.tag.outputs.tag }}
```

Note: Semver tagging (v1.2, v1.2.3) can be added later via the bake file or a pre-step that generates additional tags. For now, single tag per release is sufficient.

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/docker.yml
git commit -m "feat(ci): use docker bake for unified image builds"
```

---

### Task 4: End-to-End Verification

- [ ] **Step 1: Clean build of unified Dockerfile**

```bash
docker builder prune -f
for target in sober-api sober-agent sober-scheduler sober-web sober-cli; do
  docker build --target $target -t $target:test -f infra/docker/Dockerfile.ci . 2>&1 | tail -1
done
```

Expected: All 5 targets build. Builder stages are compiled once and reused.

- [ ] **Step 2: Verify dev compose still works (unchanged, uses per-service Dockerfiles)**

```bash
docker compose up -d --build --quiet-pull
docker compose ps --format "table {{.Name}}\t{{.Status}}"
```

Expected: All services healthy — dev workflow is unaffected.

- [ ] **Step 3: Test the app**

Send a test message through the web UI at `http://localhost:8088` and verify full round-trip (WebSocket → API → Agent → response streaming).

- [ ] **Step 4: Second build (cache hit)**

```bash
docker compose build 2>&1 | tail -5
```

Expected: Build completes in seconds — all layers cached. Only a source code change should trigger the app build stage; dependency-only changes hit the chef cook cache.

---

## Expected Build Time Improvements

| Scenario | Before (5 separate) | After (unified + cargo-chef) |
|----------|---------------------|-------------------------------|
| Clean build | ~60 min | ~15 min (1x dep compile) |
| Cargo.lock change | ~60 min | ~15 min (1x dep recompile) |
| Source-only change | ~25 min | ~5 min (deps cached, 1x app compile) |
| No backend change | ~5 min | ~1 min (all layers cached) |
| CI (GitHub Actions) | 5 parallel jobs × ~15 min | 1 job × ~15 min |
