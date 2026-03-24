#!/usr/bin/env bash
# Claude Code Stop hook: remind about uncommitted code changes.
# Checks git status and outputs a systemMessage if backend/frontend files are dirty.
set -uo pipefail

CHANGES=$(git diff --name-only 2>/dev/null; git diff --cached --name-only 2>/dev/null)
CHANGES=$(echo "$CHANGES" | sort -u | grep -v '^$')
[ -z "$CHANGES" ] && exit 0

HAS_RUST=0
HAS_FE=0
while IFS= read -r f; do
    case "$f" in
        backend/*.rs|backend/Cargo.*) HAS_RUST=1 ;;
        frontend/*.svelte|frontend/*.ts|frontend/*.js) HAS_FE=1 ;;
    esac
done <<< "$CHANGES"

[ "$HAS_RUST" -eq 0 ] && [ "$HAS_FE" -eq 0 ] && exit 0

MSG="Uncommitted code changes detected."
[ "$HAS_RUST" -eq 1 ] && MSG="$MSG Backend: verify with 'cd backend && cargo clippy -q -- -D warnings && cargo test --workspace -q'."
[ "$HAS_FE" -eq 1 ] && MSG="$MSG Frontend: verify with 'cd frontend && pnpm check && pnpm test --silent'."

jq -n --arg msg "$MSG" '{systemMessage: $msg}'
