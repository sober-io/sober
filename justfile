# Sober — Task Runner
# Run `just --list` to see available commands.
# Run `just setup` after cloning or creating a new worktree.

# One-time setup: configure git hooks
setup:
    git config core.hooksPath .githooks

# Start backend and frontend in watch mode
dev:
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'kill 0' EXIT
    (cd backend && cargo watch -q -c -x 'run -q -p sober-api') &
    (cd frontend && pnpm dev) &
    wait

# Production build of backend and frontend
build:
    cd backend && cargo build -q --release
    cd frontend && pnpm build --silent

# Run unit tests (no Docker required)
test:
    cd backend && cargo test --workspace -q
    cd frontend && pnpm test --silent

# Run integration tests (starts Docker services, waits for health, runs ignored tests)
test-integration:
    #!/usr/bin/env bash
    set -euo pipefail
    docker compose up -d
    echo "Waiting for services..."
    docker compose exec -T postgres pg_isready -U sober -q --timeout=30
    until curl -sf http://localhost:6334/readyz > /dev/null 2>&1; do sleep 1; done
    echo "Services ready. Running integration tests..."
    cd backend && cargo test --workspace -q -- --ignored
    echo "Integration tests passed."

# Run all tests (unit + integration)
test-all:
    just test
    just test-integration

# Type-check and lint everything
check:
    cd backend && cargo check -q
    cd backend && cargo clippy -q -- -D warnings
    cd frontend && pnpm check

# Check formatting
fmt:
    cd backend && cargo fmt --check -q
    cd frontend && pnpm format

# Lint only
lint:
    cd backend && cargo clippy -q -- -D warnings
    cd frontend && pnpm lint

# Audit dependencies for known vulnerabilities
audit:
    cd backend && cargo audit -q

# Regenerate Grafana dashboards and Prometheus alert rules from metrics.toml files
dashboards:
    cd tools/dashboard-gen && cargo run -q -- \
        --input ../../backend/crates \
        --dashboards-output ../../infra/grafana/dashboards/generated \
        --alerts-output ../../infra/prometheus/alerts/generated

# Start observability stack (Prometheus, Tempo, Grafana)
observability-up:
    docker compose up -d prometheus tempo grafana

# Stop observability stack
observability-down:
    docker compose stop prometheus tempo grafana

# Build documentation site
docs-build:
    cd docs/book && mdbook build

# Serve documentation locally with hot reload
docs-serve:
    cd docs/book && mdbook serve --open

# Run production stack (pre-built images from GHCR)
docker:
    docker compose -f docker-compose.prod.yml up -d

# Run development stack (builds from source)
docker-dev:
    docker compose up -d --build

# Stop all Docker services
docker-down:
    docker compose -f docker-compose.prod.yml down 2>/dev/null; docker compose down 2>/dev/null; true

# Tag and push a release manually (e.g., just release 0.2.0)
# Note: PRs merged to main are auto-tagged and released via CI
release version:
    #!/usr/bin/env bash
    set -euo pipefail
    TAG="v{{ version }}"
    if git rev-parse "$TAG" >/dev/null 2>&1; then
        echo "Error: tag $TAG already exists" >&2
        exit 1
    fi
    echo "Creating release $TAG..."
    git tag -a "$TAG" -m "Release $TAG"
    git push origin "$TAG"
    echo "Pushed $TAG — GitHub Actions will build and publish the release."
