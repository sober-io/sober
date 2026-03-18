# #032: Split SOUL.md into Structured Prompt Directory — Plan

## Phase 1: Types and infrastructure ✅

1. Create `frontmatter.rs` — `InstructionCategory`, `Visibility`, `InstructionFrontmatter`,
   `parse_frontmatter()`
2. Create `instructions.rs` — `InstructionFile` struct, `filter_and_sort()`
3. Create `references.rs` — `ReferenceResolver` with `@path` expansion
4. Update `error.rs` — add `InstructionLoadFailed`, `FrontmatterParseFailed`,
   `ReferenceResolutionFailed` variants
5. Update `Cargo.toml` — add `serde_yml`
6. Update `lib.rs` — add module declarations and re-exports

## Phase 2: Create instruction files ✅

1. Create `backend/crates/sober-mind/instructions/` directory
2. Split SOUL.md content into 10 individual `.md` files with frontmatter:
   `soul.md`, `safety.md`, `memory.md`, `reasoning.md`, `evolution.md`,
   `tools.md`, `workspace.md`, `artifacts.md`, `extraction.md`, `internal-tools.md`

## Phase 3: Loader and resolver implementation ✅

1. Implement `InstructionLoader` in `instructions.rs`:
   - Embed base files via `include_str!("../instructions/*.md")`
   - `new(user_dir)` — parses embedded base, loads user files, merges
   - `cached()` — returns pre-cached base+user instructions
   - `load_workspace(dir)` — on-demand workspace loading
   - `merge_with_workspace()` — merge workspace into base+user
   - Protection: workspace cannot extend `safety.md`
2. `ReferenceResolver` already implemented in Phase 1

## Phase 4: Assembly pipeline switchover ✅

1. Update `Mind` struct — add `InstructionLoader`, `workspace_cache: RwLock<HashMap>`
2. Update `SoulResolver` — base from embedded `include_str!`, no filesystem path
3. Rewrite `Mind::assemble()` — new pipeline: resolve soul → get instructions →
   replace soul.md body → filter visibility → sort → concatenate → tools
4. Rewrite `Mind::assemble_autonomous_prompt()` — same new pipeline
5. Remove `MEMORY_EXTRACTION_INSTRUCTIONS` const (now in `extraction.md`)
6. Simplify `access.rs` — remove INTERNAL markers, add `is_visible()` predicate
7. Update `sober-agent/src/main.rs` — no base path, pass user_dir
8. Update `infra/docker/Dockerfile.agent` — remove `COPY backend/soul/`
9. `git rm -r backend/soul/`
10. Update all tests

## Phase 5: Documentation and cleanup ✅

1. Update ARCHITECTURE.md — structured instructions, new soul.md resolution
2. Update CLAUDE.md — remove `soul/` from repo structure
3. Version bump: sober-mind 0.4.0 → 0.5.0, sober-agent 0.10.0 → 0.11.0

## Verification

- `cargo build -q -p sober-mind -p sober-agent` ✅
- `cargo test -p sober-mind -q` — 72 passed ✅
- `cargo test -p sober-agent -q` — 61 passed ✅
- `cargo clippy -q -p sober-mind -p sober-agent -- -D warnings` ✅
