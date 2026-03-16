# Design 028: Recall & Remember Memory Tools

## Context

The agent's memory system is fully implemented — hybrid dense+BM25 search in
Qdrant, context loading with token budgets, background ingestion of
conversation chunks. However, memory lookups are unreliable because:

1. **The agent doesn't find relevant memories** — the raw user message
   embedding doesn't match stored memory embeddings well enough (semantic gap).
2. **The agent doesn't know when to look** — passive context loading only
   searches using the current message's embedding, missing relevant memories
   that would be found with a better-crafted query.

## Solution

Two new LLM-invocable tools that give the agent explicit control over memory
operations:

- **`recall`** — active memory search with a crafted query
- **`remember`** — explicit storage of structured facts/preferences/knowledge

Both are thin wrappers around existing `MemoryStore` methods. No new
infrastructure needed.

## Decisions

- Tools are LLM-invoked, registered in the existing tool registry
- `recall` is `context_modifying: false` (results are in the tool output
  message — no need for a full context rebuild)
- `remember` is `context_modifying: false` (fire-and-forget storage)
- No deduplication — pruning handles stale/low-importance duplicates
- Usage guidance lives in tool descriptions, not SOUL.md
- `user_id` is embedded into the tool structs at construction time (per-request)
- Chunk type stored as integer in Qdrant — filter with integer match, not keyword
- BM25 vector generation is handled internally by `MemoryStore` — tools only
  provide the dense embedding vector

---

## 1. `recall` Tool

**Name:** `recall`

**Description (for LLM):** Search your memory for stored facts, preferences,
code, skills, and conversation history. Use this when the user asks about
something they've told you before, when you need context from past
conversations, when answering questions that might depend on stored
facts/preferences, or before saying "I don't know" — check memory first.

**Input schema:**
```json
{
  "query": "string (required) — search query, crafted for semantic relevance",
  "chunk_type": "string (optional) — filter: fact, conversation, skill, preference, code, soul",
  "scope": "string (optional) — filter: user, system. Default: user",
  "limit": "integer (optional) — max results. Default: 10, max: 20"
}
```

**Behavior:**
1. Embed `query` via `LlmEngine::embed()`
2. Call `MemoryStore::search()` with the dense vector. `search()` handles
   BM25 internally. Apply `chunk_type` filter as an integer payload condition
   (chunk types are stored as `u8` in Qdrant). Map `scope` to the appropriate
   collection: `"user"` → `user_{user_id}`, `"system"` → `system`
3. Format results: each hit shows chunk type, content, importance score,
   creation date
4. Apply retrieval boost to returned results (existing fire-and-forget pattern)
5. Return formatted results as tool output

**`context_modifying: false`** — the recall results are already available to
the LLM as the tool output in the message history. A context rebuild would
redundantly re-embed the original message and re-run passive retrieval.

**Key difference from passive retrieval:** The LLM formulates a targeted query
(e.g., "user's programming language preferences") rather than using the raw
user message embedding. This bridges the semantic gap between indirect
questions and stored memories.

---

## 2. `remember` Tool

**Name:** `remember`

**Description (for LLM):** Store a fact, preference, skill, code snippet, or
other knowledge in memory for future recall. Use this when the user shares
personal facts or preferences, when you learn something useful for future
conversations, when the user explicitly asks you to remember something, or
after extracting key decisions/outcomes from a conversation.

**Input schema:**
```json
{
  "content": "string (required) — the information to remember",
  "chunk_type": "string (required) — fact, preference, skill, code, conversation, soul",
  "importance": "number (optional) — 0.0-1.0, default varies by chunk type"
}
```

**Default importance by chunk type:**

| Type | Default | Rationale |
|------|---------|-----------|
| `soul` | 0.9 | Near-permanent identity traits |
| `preference` | 0.8 | Long-lasting user preferences |
| `fact` | 0.7 | Important but may change |
| `skill` | 0.7 | Learned capabilities |
| `code` | 0.6 | Useful but context-dependent |
| `conversation` | 0.5 | Same as background ingestion |

**Behavior:**
1. Embed `content` via `LlmEngine::embed()`
2. Call `MemoryStore::store()` with the dense vector, chunk type, importance
   (or default by type), scope = user (`user_{user_id}` collection),
   `decay_at = now + MemoryConfig::decay_half_life_days`
3. `store()` handles BM25 vector generation internally
4. Return confirmation with stored memory ID

**`context_modifying: false`** — storage doesn't affect the current context.

---

## 3. Implementation Details

### 3.1 User context

Both tools need `user_id` to determine the Qdrant collection (`user_{id}`)
and scope. The `user_id` is embedded into the tool structs at construction
time — when the agent builds its tool set for a request, it passes the
caller's `UserId` to `RecallTool::new()` and `RememberTool::new()`. This
follows the pattern used by `SchedulerTools` which receives `caller_user_id`
at construction.

### 3.2 Scope mapping

- `"user"` (default) → `ScopeId::from_uuid(user_id.as_uuid())` → collection
  `user_{user_id}`
- `"system"` → `ScopeId::GLOBAL` → collection `system`

### 3.3 Tool structs

**New:** `backend/crates/sober-agent/src/tools/memory.rs`

Two structs implementing the `Tool` trait:
- `RecallTool` — holds `Arc<MemoryStore>`, `Arc<LlmEngine>`, `UserId`
- `RememberTool` — holds `Arc<MemoryStore>`, `Arc<LlmEngine>`, `UserId`,
  `MemoryConfig`

Each implements `Tool::execute(input: Value) -> BoxToolFuture`.

### 3.4 Tool registration

**Modify:** `main.rs` (or wherever `ToolRegistry` builtins are constructed)

Add `RecallTool` and `RememberTool` to the `builtins` vec, same pattern as
existing tools. They are constructed per-request with the caller's `user_id`.

### 3.5 Chunk type filtering in Qdrant

`MemoryStore::search()` needs an optional `chunk_type: Option<u8>` parameter.
When set, add a `FieldCondition::match_value` (integer match) on the
`chunk_type` payload field to the Qdrant search filter. The tool maps the
string `"fact"` → `ChunkType::Fact as u8`, etc.

### 3.6 No database changes

All memory storage is in Qdrant. No SQL migrations needed.

### 3.7 No frontend changes

These are agent-internal tools. The frontend already renders tool calls via
`ToolCallDisplay.svelte` — `recall` and `remember` will appear there
automatically.

### 3.8 Inline memory extraction (replaces background ingestion)

The existing `spawn_memory_ingestion_static` stores entire raw messages as
`Conversation` chunks — producing noisy, low-quality memories. Replace with
**inline extraction**: the system prompt instructs the LLM to append a
structured `<memory_extractions>` block to its response containing concise
facts extracted from the conversation turn.

**System prompt addition** (in `sober-mind` prompt assembly):
```
After your response, if the conversation contained any facts, preferences,
decisions, or other information worth remembering for future conversations,
append a memory extraction block. If nothing is worth remembering, omit it.

<memory_extractions>
[{"content": "concise fact", "type": "fact|preference|skill|code"}]
</memory_extractions>
```

**Agent-side processing:**
1. After receiving the full assistant response, parse and strip
   `<memory_extractions>...</memory_extractions>` from the content
2. For each extraction, store via `MemoryStore::store()` with the appropriate
   `ChunkType` and default importance for that type
3. The stripped response (without the XML block) is what gets stored as the
   message and shown to the user

**Remove `spawn_memory_ingestion_static`** — no more raw conversation chunk
storage. Memory is now either:
- Explicitly stored via `remember` tool (LLM-initiated)
- Automatically extracted via inline `<memory_extractions>` (zero extra cost)

**Benefits:**
- Zero extra LLM calls — extraction is part of the normal response
- Higher quality — the model understands full context and extracts concise facts
- Properly typed — facts, preferences, skills instead of everything being "conversation"
- The `remember` tool serves as a manual override when the model wants to store
  something outside the normal response flow

### 3.9 Testing

Unit tests for the tool input parsing, chunk type mapping, and memory extraction
parsing logic. Integration tests (requiring Docker + Qdrant) for end-to-end
store/recall cycles.
