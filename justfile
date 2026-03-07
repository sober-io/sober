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

# Run all tests
test:
    cd backend && cargo test --workspace -q
    cd frontend && pnpm test --silent

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
