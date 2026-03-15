# Plan 028: Recall & Remember Memory Tools

## Goal

Give the agent explicit `recall` and `remember` tools so it can actively
search and store memories instead of relying solely on passive context loading.

## Changes

### Backend

1. **Extend `MemoryStore::search` with chunk type filter**
   - `sober-memory/src/store/memory_store.rs` — add `chunk_type: Option<u8>`
     parameter to `search()`. When set, add `FieldCondition::match_value`
     (integer match) on the `chunk_type` payload field to the Qdrant filter.
     Update all existing callers to pass `None`

2. **New memory tool structs**
   - New `sober-agent/src/tools/memory.rs`:
     - `RecallTool` struct: holds `Arc<MemoryStore>`, `Arc<LlmEngine>`,
       `UserId`. Implements `Tool` trait. `execute()` parses query/chunk_type/
       scope/limit from input JSON, embeds query, calls `search()` with
       filters, formats results, applies retrieval boost, returns formatted
       output
     - `RememberTool` struct: holds `Arc<MemoryStore>`, `Arc<LlmEngine>`,
       `UserId`, `MemoryConfig`. Implements `Tool` trait. `execute()` parses
       content/chunk_type/importance from input JSON, embeds content, calls
       `store()` with chunk type + importance (default by type if not
       specified) + `decay_at` from `MemoryConfig::decay_half_life_days`,
       returns confirmation with memory ID
   - Both tools: `context_modifying: false`
   - Tool descriptions include usage guidance

3. **Register tools in builtins**
   - Add `RecallTool` and `RememberTool` to the `builtins` vec where
     `ToolRegistry` is constructed. Constructed per-request with caller's
     `UserId`, following the `SchedulerTools` pattern

## Acceptance Criteria

- Agent can invoke `recall` with a crafted query and receive formatted results
- Agent can invoke `remember` to store facts/preferences/skills/code
- Chunk type filter works as integer match in Qdrant queries
- Scope maps correctly: "user" → user collection, "system" → system collection
- Default importance values applied per chunk type when not specified
- Retrieval boost applied to recall results
- Tools appear in `tool_definitions()` with descriptive usage guidance
- `cargo build -q -p sober-memory && cargo clippy -q -p sober-memory -- -D warnings`
- `cargo build -q -p sober-agent && cargo clippy -q -p sober-agent -- -D warnings`
- `cargo test -p sober-agent -q`
