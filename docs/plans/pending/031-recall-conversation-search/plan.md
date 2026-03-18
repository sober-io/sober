# Plan 031: Recall Tool — Conversation Search

## Context

The `recall` tool searches Qdrant for stored memories (facts, preferences, skills, code). But conversations live only in PostgreSQL and are unreachable once they scroll out of the recent-messages window. The agent needs to search past conversations for decisions, discussions, and context that was said but not extracted into memory.

**Solution:** Extend `recall` with a `source` parameter — `"memory"` (default, Qdrant) or `"conversations"` (PostgreSQL full-text search). Dual tsvector columns (`simple` + `english`) for multilingual support.

**Design spec:** `docs/plans/pending/031-recall-conversation-search/design.md`

---

## Step 1: Migration — add tsvector columns + GIN indexes

**File:** `backend/migrations/<timestamp>_add_message_search_vectors.sql` (new)

```sql
ALTER TABLE messages
  ADD COLUMN search_vector_simple tsvector
    GENERATED ALWAYS AS (to_tsvector('simple', content)) STORED,
  ADD COLUMN search_vector_english tsvector
    GENERATED ALWAYS AS (to_tsvector('english', content)) STORED;

CREATE INDEX idx_messages_search_simple ON messages USING GIN (search_vector_simple);
CREATE INDEX idx_messages_search_english ON messages USING GIN (search_vector_english);
```

Generated columns — PG maintains automatically. Existing rows populated on migration.

**Verify:** Run migration against Docker PG, confirm columns + indexes with `\d messages`.

---

## Step 2: Add `MessageSearchHit` domain type

**File:** `backend/crates/sober-core/src/types/domain.rs`
- Add `MessageSearchHit` struct after `Message` (~line 128):
  - Fields: `message_id: MessageId`, `conversation_id: ConversationId`, `conversation_title: Option<String>`, `role: MessageRole`, `content: String`, `score: f32`, `created_at: DateTime<Utc>`
  - Derives: `Debug, Clone, Serialize, Deserialize`

**File:** `backend/crates/sober-core/src/types/mod.rs`
- Add `MessageSearchHit` to the domain re-export line

---

## Step 3: Add `Display` for `MessageRole`

**File:** `backend/crates/sober-core/src/types/enums.rs`
- `MessageRole` has no `Display` impl. Add one so the tool can format roles in output:
  ```rust
  impl std::fmt::Display for MessageRole {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          match self {
              Self::User => f.write_str("user"),
              Self::Assistant => f.write_str("assistant"),
              Self::System => f.write_str("system"),
              Self::Tool => f.write_str("tool"),
              Self::Event => f.write_str("event"),
          }
      }
  }
  ```

---

## Step 4: Add `search_by_user` to `MessageRepo` trait + Clone bound

**File:** `backend/crates/sober-core/src/types/repo.rs`
- Add after `get_by_id` (line ~239):
  ```rust
  fn search_by_user(
      &self,
      user_id: UserId,
      query: &str,
      conversation_id: Option<ConversationId>,
      limit: i64,
  ) -> impl Future<Output = Result<Vec<MessageSearchHit>, AppError>> + Send;
  ```

**File:** `backend/crates/sober-core/src/types/agent_repos.rs`
- Change `type Msg: MessageRepo;` → `type Msg: MessageRepo + Clone;`
  - Needed so bootstrap can clone the repo into `RecallTool`
  - Follows existing pattern: `Secret: SecretRepo + Clone`, `Audit: AuditLogRepo + Clone`

**Note:** Workspace won't compile until Step 5 is done (missing impl). Do Steps 2-5 together.

---

## Step 5: Implement in `sober-db`

**File:** `backend/crates/sober-db/src/repos/messages.rs`
- Add `#[derive(Clone)]` to `PgMessageRepo` (holds `PgPool` which is `Arc`-based, zero-cost clone)
- Implement `search_by_user`:
  - Private `MessageSearchHitRow` struct with `sqlx::FromRow`
  - `From<MessageSearchHitRow> for MessageSearchHit` conversion
  - SQL: dual tsvector search with `GREATEST(ts_rank_cd(...))`, joins to conversations for `user_id` + title, optional `conversation_id` filter
  - Use `websearch_to_tsquery` for both `'english'` and `'simple'` configs

**File:** `backend/crates/sober-db/src/rows.rs`
- Alternative location for the row type if the project keeps all row types centralized here. Check existing pattern — looks like rows.rs is used. Add `MessageSearchHitRow` + `From` impl here.

**Verify:** `cargo build -q -p sober-db`

---

## Step 6: Make `RecallTool` generic, add conversation search

**File:** `backend/crates/sober-agent/src/tools/memory.rs`

Changes:
1. Add `const MAX_QUERY_LENGTH: usize = 256;`
2. `RecallTool` → `RecallTool<M: MessageRepo>`, add `messages: Arc<M>` field
3. Update constructor to accept `messages: Arc<M>`
4. `execute_inner`: validate query (empty, length), parse `source`, branch:
   - `"memory"` → existing Qdrant search (extract into `search_memory` helper)
   - `"conversations"` → new `search_conversations` method calling `self.messages.search_by_user()`
5. `search_conversations`: parse optional `conversation_id`, call repo, format results
6. Update `impl<M: MessageRepo + 'static> Tool for RecallTool<M>`
7. Update `metadata()`:
   - New description with clear source guidance
   - Add `source` and `conversation_id` to input schema
   - Mark `chunk_type` and `scope` as memory-only in descriptions

---

## Step 7: Wire in bootstrap

**File:** `backend/crates/sober-agent/src/tools/bootstrap.rs`
- In `build_static_tools()`, pass message repo when constructing `RecallTool`:
  ```rust
  Arc::new(RecallTool::new(
      ...,
      Arc::new(self.repos.messages().clone()),
  ))
  ```
- `self.repos` is the `AgentRepos` impl; `.messages()` returns `&Self::Msg`; `.clone()` works because Step 4 added `Clone` bound

**Verify:** `cargo build -q --workspace && cargo clippy -q -- -D warnings`

---

## Step 8: Regenerate sqlx offline data

```bash
cd backend && cargo sqlx prepare --workspace -q
```

Only if `.sqlx/` is in use (check if directory exists and is tracked by git).

---

## Step 9: Tests

**Integration tests** (Docker required) in `sober-db`:
- `search_by_user_returns_matching_messages` — create user, conversation, messages, search, verify hits
- `search_by_user_scoped_to_conversation` — filter by conversation_id
- `search_by_user_no_cross_user_leakage` — user A can't find user B's messages
- `search_by_user_empty_results` — query with no matches returns empty vec

**Unit tests** in `sober-agent/src/tools/memory.rs`:
- `empty_query_rejected` — validates empty string error
- `oversized_query_rejected` — validates length limit
- `unknown_source_rejected` — validates source enum
- Existing tests remain unchanged

**Verify:** `cargo test -p sober-db -q && cargo test -p sober-agent -q`

---

## Critical files

| File | Change |
|------|--------|
| `backend/crates/sober-core/src/types/domain.rs` | `MessageSearchHit` struct |
| `backend/crates/sober-core/src/types/enums.rs` | `Display` for `MessageRole` |
| `backend/crates/sober-core/src/types/repo.rs` | `search_by_user` on `MessageRepo` |
| `backend/crates/sober-core/src/types/agent_repos.rs` | `Clone` bound on `Msg` |
| `backend/crates/sober-core/src/types/mod.rs` | Re-export `MessageSearchHit` |
| `backend/crates/sober-db/src/repos/messages.rs` | `PgMessageRepo` impl + Clone |
| `backend/crates/sober-db/src/rows.rs` | `MessageSearchHitRow` + From |
| `backend/crates/sober-agent/src/tools/memory.rs` | Generic `RecallTool<M>`, source routing |
| `backend/crates/sober-agent/src/tools/bootstrap.rs` | Wire message repo |

## Verification

1. `cargo build -q --workspace`
2. `cargo clippy -q -- -D warnings`
3. `cargo test -p sober-core -q`
4. `cargo test -p sober-db -q` (Docker)
5. `cargo test -p sober-agent -q`
6. Manual: start the system, open a conversation, send some messages, then in a new conversation ask the agent to recall something from the previous one using `source: "conversations"`
