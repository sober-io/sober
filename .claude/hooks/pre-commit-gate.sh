#!/usr/bin/env bash
# Claude Code PreToolUse hook: gate git commit behind quality checks.
# Detects staged backend/frontend files and runs the relevant pipeline.
# Outputs JSON to deny the commit if checks fail.
set -uo pipefail

CMD=$(jq -r '.tool_input.command // ""')

# Only intercept git commit commands
echo "$CMD" | grep -qE '^\s*git\s+commit' || exit 0

REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
STAGED=$(git diff --cached --name-only --diff-filter=ACM 2>/dev/null)
[ -z "$STAGED" ] && exit 0

HAS_RUST=0
HAS_FE=0
while IFS= read -r f; do
    case "$f" in
        backend/*.rs|backend/Cargo.toml|backend/Cargo.lock) HAS_RUST=1 ;;
        frontend/*.svelte|frontend/*.ts|frontend/*.js|frontend/*.css|frontend/*.html) HAS_FE=1 ;;
    esac
done <<< "$STAGED"

[ "$HAS_RUST" -eq 0 ] && [ "$HAS_FE" -eq 0 ] && exit 0

ERRORS=""

if [ "$HAS_RUST" -eq 1 ]; then
    if ! (cd "$REPO_ROOT/backend" && cargo clippy -q -- -D warnings) >/dev/null 2>&1; then
        ERRORS="cargo clippy failed"
    fi
    if ! (cd "$REPO_ROOT/backend" && cargo test --workspace -q) >/dev/null 2>&1; then
        ERRORS="${ERRORS:+$ERRORS; }cargo test failed"
    fi
fi

if [ "$HAS_FE" -eq 1 ]; then
    if ! (cd "$REPO_ROOT/frontend" && pnpm check) >/dev/null 2>&1; then
        ERRORS="${ERRORS:+$ERRORS; }pnpm check failed"
    fi
    if ! (cd "$REPO_ROOT/frontend" && pnpm test --silent) >/dev/null 2>&1; then
        ERRORS="${ERRORS:+$ERRORS; }pnpm test failed"
    fi
fi

if [ -n "$ERRORS" ]; then
    jq -n --arg reason "Pre-commit checks failed: $ERRORS. Fix issues before committing." \
        '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $reason}}'
fi
