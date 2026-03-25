# 038: Agent Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite the agent's message handling to use per-conversation actors with write-ahead tool execution persistence, eliminating the concurrent-loop bug that produces orphaned tool_call_ids.

**Architecture:** Per-conversation actor with inbox channel serializes all message processing. Tool executions are persisted as first-class DB rows with status lifecycle (pending/running/completed/failed). The monolithic agent.rs splits into focused modules by lifecycle phase.

**Tech Stack:** Rust, tokio (actors/channels), sqlx (Postgres), tonic (gRPC/proto), SvelteKit (frontend), Tailwind CSS

**Spec:** `docs/plans/pending/038-agent-rewrite/design.md`

---

## File Map

### New files
| File | Responsibility |
|------|---------------|
| `backend/migrations/20260325000001_agent_rewrite.sql` | Schema migration (all 4 steps) |
| `backend/crates/sober-core/src/types/tool_execution.rs` | Domain type, enums, input structs |
| `backend/crates/sober-db/src/repos/tool_executions.rs` | `PgToolExecutionRepo` |
| `backend/crates/sober-agent/src/conversation.rs` | ConversationActor, inbox loop, idle timeout |
| `backend/crates/sober-agent/src/turn.rs` | Single LLM turn: context → prompt → stream → handle |
| `backend/crates/sober-agent/src/dispatch.rs` | Tool execution loop, write-ahead, confirmation |
| `backend/crates/sober-agent/src/history.rs` | DB → LLM message conversion |
| `backend/crates/sober-agent/src/ingestion.rs` | Background memory extraction |

### Modified files
| File | Changes |
|------|---------|
| `backend/crates/sober-core/src/types/mod.rs` | Re-export tool_execution module |
| `backend/crates/sober-core/src/types/ids.rs` | Add `ToolExecutionId` |
| `backend/crates/sober-core/src/types/enums.rs` | Add `ToolExecutionStatus`, `ToolExecutionSource` |
| `backend/crates/sober-core/src/types/domain.rs` | Update `Message` (remove tool_calls/tool_result, add reasoning) |
| `backend/crates/sober-core/src/types/input.rs` | Update `CreateMessage`, add `CreateToolExecution` |
| `backend/crates/sober-core/src/types/repo.rs` | Add `ToolExecutionRepo` trait |
| `backend/crates/sober-core/src/types/agent_repos.rs` | Add `ToolExec` associated type |
| `backend/crates/sober-db/src/repos/messages.rs` | Update for renamed table, remove tool columns |
| `backend/crates/sober-db/src/repos/agent_repos.rs` | Add `PgToolExecutionRepo` field |
| `backend/crates/sober-db/src/repos/mod.rs` | Export new repo |
| `backend/crates/sober-db/src/rows.rs` | Add `ToolExecutionRow`, update `MessageRow` (remove tool_calls/tool_result, add reasoning) |
| `backend/crates/sober-agent/src/agent.rs` | Slim down: ActorRegistry, send_message, accessors |
| `backend/crates/sober-agent/src/lib.rs` | Export new modules |
| `backend/crates/sober-agent/src/error.rs` | Add actor-related error variants |
| `backend/crates/sober-agent/src/stream.rs` | Update AgentEvent: replace ToolCallStart/Result with ToolExecutionUpdate |
| `backend/crates/sober-agent/src/grpc/agent.rs` | Route through ActorRegistry instead of tokio::spawn |
| `backend/crates/sober-agent/src/grpc/mod.rs` | Update event mapping for ToolExecutionUpdate |
| `backend/crates/sober-agent/src/grpc/tasks.rs` | Update tool call event references |
| `backend/crates/sober-agent/src/main.rs` | Graceful shutdown drain |
| `backend/proto/sober/agent/v1/agent.proto` | Replace ToolCallStart/Result with ToolExecutionUpdate |
| `backend/crates/sober-api/src/routes/messages.rs` | Join tool_executions in response |
| `backend/crates/sober-api/src/subscribe.rs` | Map ToolExecutionUpdate proto event to WebSocket |
| `frontend/src/lib/types/index.ts` | Update Message type, add ToolExecution |
| `frontend/src/lib/stores/websocket.svelte.ts` | Handle ToolExecutionUpdate event |

---

## Task 1: Database Migration

**Files:**
- Create: `backend/migrations/20260325000001_agent_rewrite.sql`

- [ ] **Step 1: Write migration SQL**

All 4 steps from design.md in a single migration file. See spec for full SQL.
Key operations:
1. Create `tool_execution_source` and `tool_execution_status` enums
2. Create `conversation_tool_executions` table
3. Migrate existing `role=tool` rows to new table (with orphan count notice)
4. Backfill tool arguments from assistant `tool_calls` JSONB
5. Rename `messages` → `conversation_messages`, `message_tags` → `conversation_message_tags`
6. Add `reasoning` column, backfill from metadata
7. Delete `role=tool` rows, drop `tool_calls`/`tool_result` columns
8. Swap `message_role` enum (remove `tool` value)
9. Add FK, indexes, UNIQUE constraint

- [ ] **Step 2: Test migration on local Docker DB**

```bash
cd backend && sqlx migrate run
```

Expected: Migration succeeds. Verify with:
```sql
SELECT COUNT(*) FROM conversation_tool_executions;
SELECT COUNT(*) FROM conversation_messages WHERE role = 'tool'; -- should be 0
\d conversation_messages  -- no tool_calls, no tool_result columns, has reasoning
\d conversation_tool_executions  -- new table with all columns
```

- [ ] **Step 3: Test migration on prod DB copy (already in local Docker)**

The faulty conversation `8f215099-e48f-4b4f-a664-cdac0d6145f9` has orphaned tool_calls.
Verify they are counted in the RAISE NOTICE and the orphaned rows are dropped cleanly.

- [ ] **Step 4: Verify migration data integrity**

`RAISE NOTICE` may be swallowed by sqlx migration runner. Verify directly:
```sql
-- Expected: matches number of non-orphaned tool rows before migration
SELECT COUNT(*) FROM conversation_tool_executions;
-- Expected: 0
SELECT COUNT(*) FROM conversation_messages WHERE role = 'tool';
-- Expected: reasoning column exists, tool_calls/tool_result gone
\d conversation_messages
```

- [ ] **Step 5: Commit migration only (sqlx prepare deferred to Task 3)**

`cargo sqlx prepare` cannot run yet — Rust types still reference `MessageRole::Tool`
and old column names. It runs after Task 3 when all Rust types are updated.

```bash
git add backend/migrations/
git commit -m "feat(db): add agent rewrite migration (#038)

Create conversation_tool_executions table, rename messages to
conversation_messages, migrate tool result data, add reasoning column."
```

---

## Task 2: Core Types — ToolExecution Domain

**Files:**
- Create: `backend/crates/sober-core/src/types/tool_execution.rs`
- Modify: `backend/crates/sober-core/src/types/ids.rs`
- Modify: `backend/crates/sober-core/src/types/enums.rs`
- Modify: `backend/crates/sober-core/src/types/mod.rs`

- [ ] **Step 1: Add ToolExecutionId**

In `ids.rs`, add:
```rust
define_id!(
    /// Unique identifier for a tool execution.
    ToolExecutionId
);
```

- [ ] **Step 2: Add enums**

In `enums.rs`, add:
```rust
/// Source of a tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(feature = "postgres", sqlx(type_name = "tool_execution_source", rename_all = "lowercase"))]
pub enum ToolExecutionSource {
    Builtin,
    Plugin,
    Mcp,
}

/// Status of a tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(feature = "postgres", sqlx(type_name = "tool_execution_status", rename_all = "lowercase"))]
pub enum ToolExecutionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}
```

- [ ] **Step 3: Create tool_execution.rs domain type**

```rust
/// A tool execution within a conversation turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecution {
    pub id: ToolExecutionId,
    pub conversation_id: ConversationId,
    pub conversation_message_id: MessageId,
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub source: ToolExecutionSource,
    pub status: ToolExecutionStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub plugin_id: Option<PluginId>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}
```

- [ ] **Step 4: Add input struct**

In `input.rs` or `tool_execution.rs`:
```rust
/// Input for creating a pending tool execution.
pub struct CreateToolExecution {
    pub conversation_id: ConversationId,
    pub conversation_message_id: MessageId,
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub source: ToolExecutionSource,
    pub plugin_id: Option<PluginId>,
}
```

- [ ] **Step 5: Add MessageWithExecutions**

In `tool_execution.rs` or `domain.rs`:
```rust
/// A message with its associated tool executions (for LLM context reconstruction).
#[derive(Debug, Clone)]
pub struct MessageWithExecutions {
    pub message: Message,
    pub tool_executions: Vec<ToolExecution>,
}
```

- [ ] **Step 6: Re-export from mod.rs**

Add `pub mod tool_execution;` to `types/mod.rs` and re-export:
`ToolExecution`, `CreateToolExecution`, `ToolExecutionId`, `ToolExecutionSource`,
`ToolExecutionStatus`, `MessageWithExecutions`.

- [ ] **Step 7: Update Message domain type**

In `domain.rs`, update `Message`:
- Remove `tool_calls: Option<serde_json::Value>`
- Remove `tool_result: Option<serde_json::Value>`
- Add `reasoning: Option<String>`

Update `CreateMessage` in `input.rs`:
- Remove `tool_calls`, `tool_result` fields

- [ ] **Step 8: Remove `Tool` from `MessageRole`**

In `enums.rs`, remove `Tool` variant from `MessageRole`. Update any match arms.

- [ ] **Step 9: Run sober-core tests**

```bash
cargo test -p sober-core -q
cargo clippy -p sober-core -q -- -D warnings
```

**Note:** The broader workspace will NOT compile at this point. `sober-db`, `sober-agent`,
and `sober-api` still reference removed fields (`tool_calls`, `tool_result`,
`MessageRole::Tool`). These are fixed in Tasks 3-11. This is expected — each subsequent
task resolves more compile errors. The error circuit breaker logic (3 consecutive failures)
in the current agent.rs will be preserved when moving to `turn.rs` in Task 9.

- [ ] **Step 10: Commit**

```bash
git commit -m "feat(core): add ToolExecution domain types, update Message (#038)

Add ToolExecutionId, ToolExecutionSource, ToolExecutionStatus enums.
Add ToolExecution domain type and CreateToolExecution input.
Remove tool_calls/tool_result from Message, add reasoning field.
Remove Tool variant from MessageRole enum."
```

---

## Task 3: ToolExecutionRepo Trait + Implementation

**Files:**
- Modify: `backend/crates/sober-core/src/types/repo.rs`
- Modify: `backend/crates/sober-db/src/rows.rs` (add `ToolExecutionRow`, update `MessageRow`)
- Create: `backend/crates/sober-db/src/repos/tool_executions.rs`
- Modify: `backend/crates/sober-db/src/repos/mod.rs`

- [ ] **Step 1: Define ToolExecutionRepo trait**

In `repo.rs`:
```rust
pub trait ToolExecutionRepo: Send + Sync {
    /// Creates a pending tool execution (write-ahead).
    fn create_pending(
        &self,
        input: CreateToolExecution,
    ) -> impl Future<Output = Result<ToolExecution, AppError>> + Send;

    /// Updates status, output, error, and timestamps.
    fn update_status(
        &self,
        id: ToolExecutionId,
        status: ToolExecutionStatus,
        output: Option<&str>,
        error: Option<&str>,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Finds incomplete (pending/running) executions for crash recovery.
    fn find_incomplete(
        &self,
        conversation_id: ConversationId,
    ) -> impl Future<Output = Result<Vec<ToolExecution>, AppError>> + Send;

    /// Finds all executions for a specific assistant message.
    fn find_by_message(
        &self,
        message_id: MessageId,
    ) -> impl Future<Output = Result<Vec<ToolExecution>, AppError>> + Send;

    /// Loads messages with their tool executions for LLM context reconstruction.
    fn list_messages_with_executions(
        &self,
        conversation_id: ConversationId,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<MessageWithExecutions>, AppError>> + Send;
}
```

- [ ] **Step 2: Add row type**

In `rows.rs`, add `ToolExecutionRow`:
```rust
#[derive(sqlx::FromRow)]
pub(crate) struct ToolExecutionRow {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub conversation_message_id: Uuid,
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub source: ToolExecutionSource,
    pub status: ToolExecutionStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub plugin_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl From<ToolExecutionRow> for ToolExecution { ... }
```

- [ ] **Step 3: Implement PgToolExecutionRepo**

In `repos/tool_executions.rs`. Key methods:
- `create_pending`: INSERT with `status='pending'`, `created_at=now()`
- `update_status`: UPDATE with conditional `started_at`/`completed_at` based on status
- `find_incomplete`: SELECT WHERE `status IN ('pending', 'running')`
- `list_messages_with_executions`: the JOIN query from the design spec

- [ ] **Step 4: Update PgMessageRepo**

In `repos/messages.rs`:
- Update table name from `messages` to `conversation_messages`
- Update column list: remove `tool_calls`, `tool_result`, add `reasoning`
- Update `CreateMessage` bindings

- [ ] **Step 5: Update MessageRow**

In `rows.rs`, update `MessageRow`:
- Remove `tool_calls`, `tool_result` fields
- Add `reasoning: Option<String>`
- Update `From<MessageRow> for Message`

- [ ] **Step 6: Update AgentRepos**

In `sober-core/src/types/agent_repos.rs`:
```rust
type ToolExec: ToolExecutionRepo;
fn tool_executions(&self) -> &Self::ToolExec;
```

In `sober-db/src/repos/agent_repos.rs`:
- Add `tool_executions: PgToolExecutionRepo` field
- Wire in `new()` and trait impl

- [ ] **Step 7: Export and test**

```bash
cargo test -p sober-db -q
cargo clippy -p sober-db -q -- -D warnings
```

- [ ] **Step 8: Prepare sqlx offline data**

Now that Rust types match the migrated schema:
```bash
cd backend && cargo sqlx prepare
```

- [ ] **Step 9: Commit**

```bash
git add backend/.sqlx/
git commit -m "feat(db): add PgToolExecutionRepo, update message repos (#038)

Implement ToolExecutionRepo trait with write-ahead create_pending,
update_status, find_incomplete, and list_messages_with_executions.
Update message repo for renamed table and removed columns."
```

---

## Task 4: Proto Changes — ToolExecutionUpdate

**Files:**
- Modify: `backend/proto/sober/agent/v1/agent.proto`

- [ ] **Step 1: Update proto**

Replace `ToolCallStart` and `ToolCallResult` in `ConversationUpdate.event` oneof:

```protobuf
message ToolExecutionUpdate {
  string id = 1;                    // tool execution UUID
  string message_id = 2;            // assistant message UUID
  string tool_call_id = 3;          // LLM-assigned tool call ID
  string tool_name = 4;
  string status = 5;                // pending, running, completed, failed, cancelled
  optional string output = 6;
  optional string error = 7;
}

message ConversationUpdate {
  string conversation_id = 1;
  oneof event {
    NewMessage new_message = 2;
    TitleChanged title_changed = 3;
    TextDelta text_delta = 4;
    ToolExecutionUpdate tool_execution_update = 5;  // replaces tool_call_start (5) and tool_call_result (6)
    ThinkingDelta thinking_delta = 7;
    ConfirmRequest confirm_request = 8;
    Done done = 9;
    Error error = 10;
  }
}
```

Note: field numbers 5 and 6 are reused — this is a breaking proto change (acceptable since API/agent deploy together).

- [ ] **Step 2: Rebuild proto**

```bash
cargo build -p sober-agent -q
```

Fix compile errors in `grpc/` modules that reference old `ToolCallStart`/`ToolCallResult` types.

- [ ] **Step 3: Update AgentEvent enum**

In `stream.rs`, replace:
```rust
ToolCallStart { name, input } → ToolExecutionUpdate { id, message_id, tool_call_id, tool_name, status, output, error }
ToolCallResult { name, output } → (removed, merged into ToolExecutionUpdate)
```

- [ ] **Step 4: Commit**

```bash
git commit -m "feat(proto): replace ToolCallStart/Result with ToolExecutionUpdate (#038)"
```

---

## Task 5: Agent Module Split — history.rs

**Files:**
- Create: `backend/crates/sober-agent/src/history.rs`

Start with the pure-function module — no agent state dependencies. This can be tested independently.

- [ ] **Step 1: Write tests for LLM message reconstruction**

```rust
#[cfg(test)]
mod tests {
    // Test: messages without tool calls → pass through
    // Test: assistant with completed tool executions → emit assistant + tool messages
    // Test: assistant with failed tool execution → emit error result
    // Test: assistant with pending tool execution → synthesize "interrupted" error
    // Test: mixed conversation (user, assistant+tools, user, assistant text)
    // Test: reasoning field echoed back on assistant messages
}
```

- [ ] **Step 2: Run tests, verify failures**

```bash
cargo test -p sober-agent -q --lib -- history
```

- [ ] **Step 3: Implement `to_llm_messages`**

Convert `Vec<MessageWithExecutions>` → `Vec<LlmMessage>`. For each message:
- `user`/`system`/`event` → map directly
- `assistant` with no tool executions → map directly, include `reasoning`
- `assistant` with tool executions → emit assistant message with `tool_calls` array, then emit one `LlmMessage::tool()` per completed/failed execution

- [ ] **Step 4: Run tests, verify passing**

```bash
cargo test -p sober-agent -q --lib -- history
```

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(agent): add history.rs — DB to LLM message conversion (#038)"
```

---

## Task 6: Agent Module Split — dispatch.rs

**Files:**
- Create: `backend/crates/sober-agent/src/dispatch.rs`

- [ ] **Step 1: Write tests for write-ahead tool execution**

Test the dispatch logic:
- Tool succeeds → pending → running → completed
- Tool fails → pending → running → failed
- Write-ahead INSERT fails → tool not executed, error returned
- Confirmation denied → cancelled

- [ ] **Step 2: Run tests, verify failures**

- [ ] **Step 3: Extract `execute_tool_calls` from agent.rs**

Move `execute_tool_calls_streaming` and `handle_confirmation` to `dispatch.rs`.
Refactor to use write-ahead pattern:
1. For each tool_call: `repos.tool_executions().create_pending(...)` before execution
2. `repos.tool_executions().update_status(id, Running, ...)` before execute
3. Execute tool (wrapped in panic catch)
4. `repos.tool_executions().update_status(id, Completed/Failed, output, error)`

- [ ] **Step 4: Run tests, verify passing**

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(agent): add dispatch.rs — write-ahead tool execution (#038)"
```

---

## Task 7: Agent Module Split — ingestion.rs

**Files:**
- Create: `backend/crates/sober-agent/src/ingestion.rs`

- [ ] **Step 1: Move `spawn_extraction_ingestion` from agent.rs**

Straightforward extraction — no behavior change.

- [ ] **Step 2: Run tests**

```bash
cargo test -p sober-agent -q
```

- [ ] **Step 3: Commit**

```bash
git commit -m "refactor(agent): extract ingestion.rs from agent.rs (#038)"
```

---

## Task 8: Agent Module Split — conversation.rs (Actor Model)

**Files:**
- Create: `backend/crates/sober-agent/src/conversation.rs`

- [ ] **Step 1: Define actor types**

```rust
pub enum InboxMessage {
    UserMessage {
        user_id: UserId,
        content: String,
        trigger: TriggerKind,
        event_tx: mpsc::Sender<Result<AgentEvent, AgentError>>,
    },
    Shutdown,
}

pub struct ConversationActor { ... }
```

- [ ] **Step 2: Implement inbox loop**

```rust
impl ConversationActor {
    pub async fn run(mut self) {
        loop {
            match tokio::time::timeout(IDLE_TIMEOUT, self.inbox.recv()).await {
                Ok(Some(InboxMessage::UserMessage { .. })) => {
                    // process turn
                }
                Ok(Some(InboxMessage::Shutdown)) | Ok(None) => break,
                Err(_) => break, // idle timeout
            }
        }
        // cleanup: remove from registry
    }
}
```

- [ ] **Step 3: Implement crash recovery on actor spawn**

On start, check for incomplete tool executions and mark them failed.

- [ ] **Step 4: Write tests**

- Inbox ordering: send 3 messages rapidly, verify processed in order
- Idle timeout: actor exits after timeout with no messages
- Shutdown: actor exits cleanly on Shutdown message

- [ ] **Step 5: Run tests**

```bash
cargo test -p sober-agent -q --lib -- conversation
```

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(agent): add conversation.rs — per-conversation actor (#038)"
```

---

## Task 9: Agent Module Split — turn.rs (Streaming)

**Files:**
- Create: `backend/crates/sober-agent/src/turn.rs`

- [ ] **Step 1: Extract `run_loop_streaming` from agent.rs into `run_turn`**

Refactor: instead of a loop in `run_loop_streaming`, the actor calls `run_turn()` which
handles one complete turn (may involve multiple LLM calls with tool execution in between).

Keep the existing loop structure but move it to `turn.rs` as a standalone function.

- [ ] **Step 2: Switch to streaming LLM calls**

Replace `llm.complete(req)` with `llm.stream(req)`:
- Forward `TextDelta` chunks to `event_tx` as they arrive
- Buffer tool_call deltas until stream completes
- Assemble final text + tool_calls from collected stream

Use `sober_llm::streaming::collect_stream_with_deltas()` or similar pattern:
process each `StreamChunk`, send text deltas, accumulate tool call pieces.

- [ ] **Step 3: Send ToolExecutionUpdate events**

Replace `ToolCallStart`/`ToolCallResult` event sends with `ToolExecutionUpdate`:
- On create_pending → send update with status=pending
- On start execution → send update with status=running
- On complete → send update with status=completed + output
- On fail → send update with status=failed + error

- [ ] **Step 4: Remove old `domain_to_llm_messages` and `sanitize_tool_call_pairs`**

These are replaced by `history::to_llm_messages`. Delete from agent.rs.

- [ ] **Step 5: Run full test suite**

```bash
cargo test -p sober-agent -q
cargo clippy -p sober-agent -q -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(agent): add turn.rs with streaming LLM + write-ahead tools (#038)"
```

---

## Task 10: Slim Down agent.rs — ActorRegistry

**Files:**
- Modify: `backend/crates/sober-agent/src/agent.rs`
- Modify: `backend/crates/sober-agent/src/grpc/agent.rs`
- Modify: `backend/crates/sober-agent/src/main.rs`
- Modify: `backend/crates/sober-agent/src/lib.rs`

- [ ] **Step 1: Add ActorRegistry to Agent struct**

```rust
use dashmap::DashMap;

struct ActorRegistry {
    actors: DashMap<ConversationId, mpsc::Sender<InboxMessage>>,
}

impl ActorRegistry {
    fn send_or_spawn(&self, conv_id, msg, /* deps for spawning */) { ... }
    async fn shutdown_all(&self) { ... }
}
```

- [ ] **Step 2: Rewrite handle_message**

Replace `tokio::spawn(run_loop_streaming)` with:
```rust
self.registry.send_or_spawn(conversation_id, InboxMessage::UserMessage { ... }, ...);
```

Return the event stream immediately.

- [ ] **Step 3: Add graceful shutdown to main.rs**

On SIGTERM: call `registry.shutdown_all()` with 30s deadline.

- [ ] **Step 4: Delete dead code from agent.rs**

Remove: `run_loop_streaming`, `execute_tool_calls_streaming`, `handle_confirmation`,
`spawn_extraction_ingestion`, `domain_to_llm_messages`, `sanitize_tool_call_pairs`,
`format_memory_context`, `LoopContext`.

These have all been moved to their new modules.

- [ ] **Step 5: Update lib.rs exports**

```rust
pub mod conversation;
pub mod turn;
pub mod dispatch;
pub mod history;
pub mod ingestion;
```

- [ ] **Step 6: Add dashmap dependency**

In `backend/crates/sober-agent/Cargo.toml`:
```toml
dashmap = "6"
```

- [ ] **Step 7: Run full agent test suite**

```bash
cargo test -p sober-agent -q
cargo clippy -p sober-agent -q -- -D warnings
```

- [ ] **Step 8: Commit**

```bash
git commit -m "feat(agent): ActorRegistry, slim agent.rs, graceful shutdown (#038)"
```

---

## Task 11: API Changes — Messages Endpoint

**Files:**
- Modify: `backend/crates/sober-api/src/routes/messages.rs`

- [ ] **Step 1: Update message list handler**

Use `ToolExecutionRepo::list_messages_with_executions()` to load messages with
inline tool executions. Serialize as:
```json
{ "data": [{ "id": "...", "role": "assistant", "content": "...", "tool_executions": [...] }] }
```

- [ ] **Step 2: Update message create handler**

Remove `tool_calls`, `tool_result` from `CreateMessage` input.
Return server-generated UUID in response.

- [ ] **Step 3: Update subscribe.rs**

In `backend/crates/sober-api/src/subscribe.rs`, update `conversation_update_to_ws`:
- Remove `ToolCallStart` / `ToolCallResult` mapping
- Add `ToolExecutionUpdate` → WebSocket message mapping

- [ ] **Step 4: Update WebSocket event broadcasting**

In API's WebSocket handler, ensure the new `ToolExecutionUpdate` events are forwarded
to connected clients.

- [ ] **Step 5: Run API tests**

```bash
cargo test -p sober-api -q
cargo clippy -p sober-api -q -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(api): inline tool executions in messages response (#038)"
```

---

## Task 12: Frontend Changes

**Files:**
- Modify: `frontend/src/lib/types/index.ts`
- Modify: `frontend/src/lib/stores/websocket.svelte.ts`
- Modify: message rendering components

- [ ] **Step 1: Update TypeScript types**

```typescript
export interface ToolExecution {
  id: string;
  tool_call_id: string;
  tool_name: string;
  input: unknown;
  source: 'builtin' | 'plugin' | 'mcp';
  status: 'pending' | 'running' | 'completed' | 'failed' | 'cancelled';
  output?: string;
  error?: string;
  started_at?: string;
  completed_at?: string;
}

export interface Message {
  id: string;
  role: 'user' | 'assistant' | 'system' | 'event';
  content: string;
  reasoning?: string;
  token_count: number;
  user_id?: string;
  metadata?: Record<string, unknown>;
  tool_executions?: ToolExecution[];
  tags?: Tag[];
  created_at: string;
}
```

Remove: `tool_calls`, `tool_result` fields. Remove `'tool'` from role union.

- [ ] **Step 2: Update WebSocket handler**

Handle `ToolExecutionUpdate` event:
- Find the assistant message by `message_id`
- Upsert the tool execution in the message's `tool_executions` array by `id`
- Update `status`, `output`, `error` fields

Remove: `ToolCallStart`, `ToolCallResult` handlers.

- [ ] **Step 3: Update message rendering**

Tool executions render inline on assistant messages instead of as separate tool message cards.
Remove the `role === 'tool'` rendering branch.

- [ ] **Step 4: Remove client-side UUID generation for messages**

Frontend sends content only; uses server-returned ID.

- [ ] **Step 5: Run frontend checks**

```bash
cd frontend && pnpm check && pnpm test --silent
```

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(frontend): tool executions inline on assistant messages (#038)"
```

---

## Task 13: Integration Test & Docker Rebuild

- [ ] **Step 1: Rebuild Docker images**

```bash
docker compose up -d --build
```

- [ ] **Step 2: Test the faulty conversation**

Open conversation `8f215099-e48f-4b4f-a664-cdac0d6145f9` in the frontend.
Send a message. Verify:
- No HTTP 400 errors
- Tool executions display inline on assistant messages
- Streaming text appears progressively

- [ ] **Step 3: Test concurrent messages**

Send a message that triggers a slow tool (e.g., `generate_plugin`).
While it's executing, send "any progress?".
Verify: second message waits, processes after first completes. No orphans.

- [ ] **Step 4: Test crash recovery**

Start a tool execution, kill the agent process mid-execution.
Restart. Verify: pending tool executions marked failed, user notified.

- [ ] **Step 5: Run full workspace tests**

```bash
cargo test --workspace -q
cargo clippy --workspace -q -- -D warnings
cd frontend && pnpm check && pnpm test --silent
```

- [ ] **Step 6: Final commit**

```bash
git commit -m "test(agent): integration tests for actor model and write-ahead (#038)"
```
