# Sober — Task Runner
# Run `just --list` to see available commands.

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
