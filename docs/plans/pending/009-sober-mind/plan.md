# 009 --- sober-mind: Implementation Plan

**Date:** 2026-03-06
**Design:** [design.md](./design.md)

**Depends on:** 003 (sober-core), 004 (sober-crypto), 006 (sober-memory)
**Must complete before:** 011 (sober-agent)

---

## Goal

Implement the agent identity and prompt assembly crate. `sober-mind` owns
SOUL.md resolution, injection detection, dynamic prompt composition, access
masks, and soul layer management.

---

## Steps

### 1. Scaffold the crate

Add `sober-mind` to the Cargo workspace. Configure `Cargo.toml` with dependencies:

- `sober-core` (path dependency)
- `sober-memory` (path dependency)
- `sober-crypto` (path dependency)
- `serde`, `serde_json`
- `tokio` (features: `fs`)
- `tracing`
- `thiserror`
- `regex`

### 2. Create module structure

```
backend/crates/sober-mind/src/
  lib.rs           -- Public API, re-exports
  error.rs         -- MindError enum
  injection.rs     -- Heuristic injection classifier
  soul.rs          -- SOUL.md loading and resolution chain
  assembly.rs      -- Prompt assembly engine
  access.rs        -- Access mask application
  layers.rs        -- Soul layer read/write via sober-memory
  evolution.rs     -- Trait evolution and confidence scoring (stub for v1)
```

### 3. Implement error.rs

Define `MindError` enum:
- `SoulLoadFailed(String)` --- SOUL.md file not found or unreadable
- `SoulMergeFailed(String)` --- workspace layer violates base constraints
- `InjectionRejected { reason: String }` --- input classified as injection
- `AssemblyFailed(String)` --- prompt assembly error
- `LayerStoreFailed(String)` --- BCF soul layer write error

Implement `From<MindError>` for `AppError`.

### 4. Implement injection.rs

Heuristic injection classifier (deterministic, no LLM calls):

- `classify_input(input: &str) -> InjectionVerdict`
- `InjectionVerdict` enum: `Pass`, `Flagged { reason }`, `Rejected { reason }`
- Detection patterns:
  - Instruction override attempts ("ignore previous instructions", "you are now...")
  - Role-play injection ("pretend you are", "act as if")
  - Context boundary manipulation (fake system message markers, delimiter injection)
  - Encoded injection attempts (base64-encoded instructions, unicode tricks)
- Uses `regex` for pattern matching. Patterns loaded from a config struct
  (extensible without code changes).

### 5. Implement soul.rs

SOUL.md loading and resolution chain:

- `SoulResolver::new(base_path, user_path, workspace_path) -> Self`
- `SoulResolver::resolve() -> Result<String, MindError>`
- Resolution chain: base (`backend/soul/SOUL.md`) -> user (`~/.sober/SOUL.md`)
  -> workspace (`./.sober/SOUL.md`)
- User layer: full override of base.
- Workspace layer: additive only. Merging appends sections; cannot remove or
  contradict base/user ethical boundaries or security rules.
- Missing layers are silently skipped (only base is required).

### 6. Implement access.rs

Access mask application:

- `apply_access_mask(prompt: &str, mask: &CallerContext) -> String`
- Strips internal-only sections from the prompt based on `TriggerKind`:
  - `Human` --- remove self-reasoning blocks, evolution state, internal tool docs
  - `Scheduler` --- full access
  - `Replica` --- scoped to delegation grants
  - `Admin` --- full read, restricted write markers
- Uses section markers in the resolved SOUL.md to identify internal-only blocks.

### 7. Implement assembly.rs

Dynamic prompt assembly engine:

- `Mind` struct holding `SoulResolver`, reference to `MemoryStore`
- `Mind::new(soul_resolver, memory_store, crypto) -> Self`
- `Mind::assemble(caller: &CallerContext, context: &Context, tools: &[ToolMetadata]) -> Result<Vec<Message>, MindError>`
  - Resolve SOUL.md chain
  - Load soul layers from memory (user/group scope)
  - Merge soul + layers
  - Apply access mask
  - Append task context
  - Append tool definitions
  - Return assembled message array (system prompt + context messages)
- `Mind::check_injection(input: &str) -> Result<InjectionVerdict, MindError>`
  - Convenience method wrapping `classify_input`

### 8. Implement layers.rs

Soul layer management via sober-memory:

- `load_soul_layers(memory: &MemoryStore, scope: ScopeId) -> Result<Vec<SoulLayer>, MindError>`
- `store_soul_layer(memory: &MemoryStore, scope: ScopeId, layer: &SoulLayer) -> Result<(), MindError>`
- `SoulLayer` struct: scope, content (key-value adaptations), confidence, updated_at
- Uses BCF `Soul` chunk type for storage

### 9. Implement evolution.rs (v1 stub)

Trait evolution is complex. v1 provides the types and interfaces but defers
autonomous evolution to post-v1:

- `TraitCandidate` struct: observation, confidence_score, source_contexts
- `EvolutionDecision` enum: `AutoAdopt`, `QueueForReview`, `Discard`
- `evaluate_candidate(candidate: &TraitCandidate) -> EvolutionDecision`
  - v1: always returns `QueueForReview` (no autonomous adoption yet)
- `EvolutionAuditEntry` struct for logging proposed changes (signed via sober-crypto)

### 10. Create base SOUL.md

Write `backend/soul/SOUL.md` with initial agent identity:
- Core values (helpfulness, honesty, security-first)
- Communication style (clear, concise, adaptive)
- Ethical boundaries (no harmful content, no credential exposure)
- Self-evolution guidelines

### 11. Wire up lib.rs

Re-export key types:
- `pub use assembly::Mind;`
- `pub use injection::{classify_input, InjectionVerdict};`
- `pub use soul::SoulResolver;`
- `pub use access::apply_access_mask;`
- `pub use layers::SoulLayer;`
- `pub use evolution::{TraitCandidate, EvolutionDecision};`
- `pub use error::MindError;`

### 12. Write unit tests

- **Injection detection:**
  - Known injection patterns are `Rejected` (test each category)
  - Benign inputs are `Pass`
  - Edge cases (partial matches) are `Flagged`
  - Empty input is `Pass`
- **SOUL.md resolution:**
  - Base only: returns base content
  - Base + user: user overrides base
  - Base + user + workspace: workspace sections appended
  - Missing base: returns `SoulLoadFailed`
  - Missing user/workspace: silently skipped
- **Access mask:**
  - Human trigger strips internal sections
  - Scheduler trigger preserves all sections
- **Prompt assembly:**
  - Assembles correct message array with system prompt + context
  - Includes tool definitions in system prompt
  - Respects access mask

### 13. Write integration tests

- Full assembly pipeline: resolve SOUL.md -> load layers -> assemble prompt
  -> verify output structure
- Injection detection with real-world examples from public injection datasets

### 14. Verify

```bash
cargo clippy -p sober-mind -- -D warnings
cargo test -p sober-mind
cargo doc -p sober-mind --no-deps
```

---

## Acceptance Criteria

- Injection classifier detects known attack patterns (instruction override,
  role-play, boundary manipulation) and returns `Rejected`.
- Benign user input passes through the classifier without false positives.
- SOUL.md resolution chain correctly merges base + user + workspace layers.
- Workspace SOUL.md cannot override ethical boundaries from base/user layers.
- Prompt assembly produces a well-formed message array with system prompt,
  context, and tool definitions.
- Access masks correctly filter internal-only content based on trigger source.
- Soul layers can be stored and retrieved via sober-memory BCF.
- Evolution module provides types and interfaces (v1 stub: no autonomous adoption).
- `cargo clippy -p sober-mind -- -D warnings` is clean.
- All public items have doc comments.
- No `.unwrap()` in library code.
