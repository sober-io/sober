# Design 031: Recall Tool — Conversation Search

## Context

The `recall` tool (plan #028) searches Qdrant for stored memories: facts,
preferences, skills, code snippets, and soul data. This works well for
user-specific knowledge the agent has explicitly extracted and stored.

However, the agent cannot search past conversation messages. Conversations live
exclusively in PostgreSQL — they are never indexed in Qdrant. The extraction
pipeline bridges some of this gap by pulling facts and preferences from
conversations, but raw message content (decisions discussed, questions asked,
technical context) is unreachable once messages scroll out of the recent-messages
window.

The agent needs a way to search past conversations for context that was *said*
but not *extracted*.

## Solution

Extend the existing `recall` tool with a `source` parameter that routes between
two backends:

- **`source: "memory"`** (default) — current Qdrant semantic search, unchanged.
- **`source: "conversations"`** — PostgreSQL full-text search over the messages
  table.

This keeps the tool count low (no new tool for the LLM to learn) while giving
the agent access to the full conversation history.

## Decisions

### Single tool, two backends

A separate `search_conversations` tool was considered but rejected. The LLM
already has a mental model for `recall` = "search my stuff." Adding a `source`
parameter is simpler than teaching it a new tool. The tool description explicitly
guides source selection.

### PostgreSQL full-text search, not Qdrant

Conversation messages stay in PostgreSQL only — no duplication into Qdrant.
Reasons:

- The extraction pipeline already bridges conversations → semantic memory by
  pulling out facts, preferences, and skills.
- Embedding every message is expensive, noisy (short messages like "ok" or
  "yes"), and individual messages often lack standalone context.
- Full-text search covers the "find where we discussed X" use case well.
- Semantic/fuzzy search is served by the existing `source: "memory"` path.

### Dual text search configurations

Two `tsvector` generated columns on the messages table:

- **`search_vector_simple`** — `to_tsvector('simple', content)`. No stemming,
  no stop words. Language-agnostic. Works for any language.
- **`search_vector_english`** — `to_tsvector('english', content)`. English
  stemming and stop words. Better recall for English text ("running" matches
  "run").

Both are queried in a single SQL statement. `GREATEST` picks the higher rank.
This lets us evaluate whether English stemming adds value over `simple` alone
for this codebase's usage patterns.

The `english` config is NOT a superset of `simple` — it can drop tokens that
match English stop words and mangle non-English words through the stemmer. Both
columns are needed for correct multilingual support.

### Repository pattern for data access

The search query goes through `MessageRepo` (new `search_by_user` method), not
raw SQL in the tool. This follows the existing architecture where all PostgreSQL
access is abstracted behind repo traits in `sober-core` with `Pg*Repo`
implementations in `sober-db`.

### RecallTool becomes generic over MessageRepo

`RecallTool<M: MessageRepo>` — follows the existing pattern used by
`AgentGrpcService<Msg, Conv, Mcp>`.

## Tool Interface

### Updated input schema

```json
{
  "type": "object",
  "properties": {
    "query": {
      "type": "string",
      "description": "Search query."
    },
    "source": {
      "type": "string",
      "enum": ["memory", "conversations"],
      "description": "Where to search. 'memory' (default) for stored knowledge about the user — facts, preferences, skills, code. 'conversations' for past conversation messages — decisions, discussions, questions, anything that was said."
    },
    "chunk_type": {
      "type": "string",
      "enum": ["fact", "conversation", "preference", "skill", "code", "soul"],
      "description": "Filter by memory type. Only applies when source is 'memory'."
    },
    "scope": {
      "type": "string",
      "enum": ["user", "system"],
      "description": "Search scope. Only applies when source is 'memory'."
    },
    "conversation_id": {
      "type": "string",
      "description": "Narrow search to a specific conversation. Only applies when source is 'conversations'."
    },
    "limit": {
      "type": "integer",
      "description": "Maximum results (default: 10, max: 20)."
    }
  },
  "required": ["query"]
}
```

### Updated tool description

```
Search your memory or past conversations.

source: "memory" (default) — Search stored knowledge about the user: personal
facts, preferences, learned skills, code snippets. Use when looking for
something you learned and stored about this user.

source: "conversations" — Search past conversation messages. Use for anything
discussed previously: decisions made, questions asked, technical discussions,
context from past interactions. This is your go-to when the user references
something from a past conversation, or when looking for context that isn't a
personal fact or preference.

Rule of thumb: if it's a user-specific trait or fact you extracted, use memory.
If it's something that was said in a conversation, use conversations.

You MUST use this tool proactively:
- At the START of every new conversation to load relevant context
- Whenever the user references something from the past
- Before answering questions that might depend on stored knowledge
- Before saying "I don't know" — always check memory first
```

## Data Model

### New migration

```sql
ALTER TABLE messages
  ADD COLUMN search_vector_simple tsvector
    GENERATED ALWAYS AS (to_tsvector('simple', content)) STORED,
  ADD COLUMN search_vector_english tsvector
    GENERATED ALWAYS AS (to_tsvector('english', content)) STORED;

CREATE INDEX idx_messages_search_simple ON messages
  USING GIN (search_vector_simple);
CREATE INDEX idx_messages_search_english ON messages
  USING GIN (search_vector_english);
```

Generated columns — PostgreSQL maintains them automatically on INSERT/UPDATE.

### New domain type

```rust
pub struct MessageSearchHit {
    pub message_id: MessageId,
    pub conversation_id: ConversationId,
    pub conversation_title: Option<String>,
    pub role: MessageRole,
    pub content: String,
    pub score: f32,
    pub created_at: DateTime<Utc>,
}
```

### Search query

```sql
SELECT m.id, m.conversation_id, c.title, m.role, m.content, m.created_at,
       GREATEST(
           ts_rank_cd(m.search_vector_english, websearch_to_tsquery('english', $1)),
           ts_rank_cd(m.search_vector_simple, websearch_to_tsquery('simple', $1))
       ) AS rank
FROM messages m
JOIN conversations c ON c.id = m.conversation_id
WHERE c.user_id = $2
  AND (m.search_vector_english @@ websearch_to_tsquery('english', $1)
       OR m.search_vector_simple @@ websearch_to_tsquery('simple', $1))
  AND ($3::uuid IS NULL OR m.conversation_id = $3)
ORDER BY rank DESC
LIMIT $4
```

## Changes Summary

| Layer | Change |
|-------|--------|
| Migration | Add two `tsvector` generated columns + GIN indexes to `messages` |
| sober-core | Add `MessageSearchHit` type, add `search_by_user` to `MessageRepo` |
| sober-db | Implement `search_by_user` on `PgMessageRepo` |
| sober-agent | Make `RecallTool` generic over `M: MessageRepo`, add `source`/`conversation_id` params, route to repo |
| sober-agent | Update tool description with source guidance |
| bootstrap | Pass `Arc<PgMessageRepo>` to `RecallTool` constructor |
| sqlx | Regenerate `.sqlx/` for offline mode |

No changes to `RememberTool`, `MemoryStore`, Qdrant, context loader, or any
other crate.
