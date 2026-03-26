# Docker Build Optimization — Design

## Problem

CI Docker image builds take ~1 hour. The project has 5 service images (`sober-api`, `sober-agent`, `sober-scheduler`, `sober-web`, `sober-cli`), each with its own Dockerfile that independently compiles the entire Rust dependency tree. Since all 5 crates share ~90% of dependencies, this means ~40 minutes of dependency compilation happens 5 times.

## Root Cause

Each `Dockerfile.<service>` runs `cargo build --release -p <crate>` in its own builder stage. Even with BuildKit cache mounts (per-service target cache IDs), a `Cargo.lock` change invalidates all 5 caches independently. There's no shared compilation across services.

## Solution

Add a single `Dockerfile.ci` with:

1. **cargo-chef** for dependency caching — separates dependency compilation (slow, cacheable) from application code compilation (fast, changes often)
2. **One builder stage** that compiles all 5 binaries in a single `cargo build --release`
3. **Named runtime stages** (`AS sober-api`, `AS sober-agent`, etc.) that each copy their binary from the shared builder

The existing per-service Dockerfiles remain for local development (`docker compose up --build`). `docker-compose.yml` is unchanged.

## CI Integration

The GitHub Actions Docker workflow (`docker.yml`) switches from a 5-job matrix (each building a separate Dockerfile) to `docker buildx bake` using a `docker-bake.hcl` config that defines all 5 targets pointing at `Dockerfile.ci`. Bake shares the builder layers across all targets in a single job.

## Cache Behavior

| Layer | Invalidated by | Rebuild cost |
|-------|---------------|-------------|
| Chef install + protoc | Base image update | ~2 min |
| Cook (dependency compilation) | `Cargo.lock` change | ~12 min |
| Build (application code) | Any `.rs` file change | ~3 min |
| Runtime stages | Binary or runtime deps change | ~30s each |

## Expected Improvement

| Scenario | Before | After |
|----------|--------|-------|
| Clean build | ~60 min | ~15 min |
| Cargo.lock change | ~60 min | ~15 min |
| Source-only change | ~25 min | ~5 min |
| No backend change | ~5 min | ~1 min |

## Scope

- **In scope:** `Dockerfile.ci`, `docker-bake.hcl`, `.github/workflows/docker.yml`
- **Unchanged:** Per-service Dockerfiles, `docker-compose.yml`, `docker-compose.prod.yml`
- **Not in scope:** CI test workflow, release workflow, binary packaging
