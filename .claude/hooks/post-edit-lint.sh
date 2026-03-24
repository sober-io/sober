#!/usr/bin/env bash
# Claude Code PostToolUse hook: lint edited files in background.
# Rust files → cargo clippy on the affected crate.
# Frontend files → eslint on the specific file.
# Exits 2 on errors to wake the model (asyncRewake).
set -uo pipefail

FILE=$(jq -r '.tool_input.file_path // ""')
[ -z "$FILE" ] && exit 0

REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)

case "$FILE" in
    *.rs)
        # Walk up to find the crate Cargo.toml
        DIR=$(dirname "$FILE")
        CRATE=""
        while [ "$DIR" != "/" ] && [ "$DIR" != "." ]; do
            if [ -f "$DIR/Cargo.toml" ]; then
                CRATE=$(grep -m1 '^name\s*=' "$DIR/Cargo.toml" | sed 's/.*"\(.*\)"/\1/')
                break
            fi
            DIR=$(dirname "$DIR")
        done
        [ -z "$CRATE" ] && exit 0

        OUTPUT=$(cd "$REPO_ROOT/backend" && cargo clippy -p "$CRATE" -q -- -D warnings 2>&1) && exit 0
        jq -n --arg ctx "clippy errors in $CRATE after editing $(basename "$FILE"):\n$OUTPUT" \
            '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $ctx}}'
        exit 2
        ;;
    *.svelte|*.ts|*.js)
        # Only lint files inside frontend/
        case "$FILE" in
            */frontend/*)
                OUTPUT=$(cd "$REPO_ROOT/frontend" && npx eslint --no-warn-ignored "$FILE" 2>&1) && exit 0
                jq -n --arg ctx "eslint errors after editing $(basename "$FILE"):\n$OUTPUT" \
                    '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $ctx}}'
                exit 2
                ;;
        esac
        ;;
esac

exit 0
