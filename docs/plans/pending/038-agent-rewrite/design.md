# 038: Agent Rewrite — Actor Model, Write-Ahead Persistence, Module Split

## Problem

The agent's monolithic `agent.rs` (2,088 lines) has a structural reliability bug: tool
results are pushed to in-memory state before DB persistence, and concurrent `tokio::spawn`
calls for the same conversation corrupt the message history. This produces orphaned
`tool_call_id` entries that cause HTTP 400 errors from the LLM API.

Root cause verified in production: every orphaned tool_call corresponds to a user message
arriving during slow tool execution (e.g., `fetch_url`, `generate_plugin`), which spawns a
second agent loop racing on the same conversation.

## Goals

1. **Eliminate tool_call orphans by design** — not by cleanup heuristics
2. **Sequential message processing per conversation** — no concurrent loops
3. **Crash recovery** — interrupted tool executions are detectable and recoverable
4. **Split agent.rs** into focused modules (~300 lines each)
5. **Clean data model** — tool executions as first-class entities, not JSONB hacks
6. **Backend-generated message IDs** — no frontend UUID generation

7. **Streaming LLM responses** — send `TextDelta` events as tokens arrive

## Non-Goals

- Parallel tool execution within a single turn (keep sequential for now)

---

## Architecture

### Conversation Actor Model

Each conversation gets a long-lived tokio task that processes messages sequentially through
an inbox channel. The `Agent` struct manages actor lifecycle via a registry.

```
HandleMessage RPC → Agent.send_message(conv_id, content)
                        ↓
                 ActorRegistry.get_or_spawn(conv_id)
                        ↓
                 inbox_tx.send(InboxMessage::UserMessage { ... })
                        ↓
            ┌─── ConversationActor (long-lived task) ───┐
            │                                            │
            │  loop { match inbox_rx.recv_timeout() {    │
            │    UserMessage → run_turn(...)             │
            │    Timeout     → break (idle cleanup)      │
            │  }}                                        │
            └────────────────────────────────────────────┘
```

**Actor lifecycle:**
- Spawned on first message to a conversation
- Lives until idle timeout (5 minutes with no messages)
- On idle timeout, actor exits and is removed from the registry
- Next message spawns a fresh actor
- On agent process restart, actors are re-spawned on demand

**Graceful shutdown (SIGTERM):**
The agent process drains the actor registry on shutdown:
1. Send `Shutdown` to all active actors via their inbox channels
2. Wait for actors to finish their current turn (with a deadline, e.g., 30 seconds)
3. After deadline, remaining actors are dropped — their in-flight tool executions
   stay as `pending`/`running` in DB and are recovered on next startup

**InboxMessage enum:**
- `UserMessage { user_id, content, trigger, event_tx }` — normal message processing
- `Shutdown` — graceful stop (sent by registry during SIGTERM drain)

The `event_tx` channel is passed per-message so each caller gets their own response stream.
When the actor finishes processing message N, the `event_tx` for N is dropped (closing
the caller's stream), and the actor picks up message N+1 from the inbox.

**Concurrency guarantee:** One actor per conversation. One message processed at a time.
The inbox channel provides natural backpressure. No mutex, no races, no orphans from
concurrent loops.

### API / Scheduler Impact

The gRPC boundary is **unchanged**. `HandleMessage` is still a unary RPC that returns an ack.
`SubscribeConversationUpdates` still streams events. The only change is internal:

```
Current:  HandleMessage → tokio::spawn(run_loop_streaming)     ← races possible
Proposed: HandleMessage → actor_registry.send(conv_id, msg)    ← queued, sequential
```

The scheduler uses the same `HandleMessage` RPC with `trigger: Scheduler`. The actor treats
scheduler messages identically — it just excludes the scheduler tool from available tools
(existing behavior). No changes to the scheduler crate or proto files.

---

## Data Model

### Schema Changes

Two tables replace the current single `messages` table for tool-related data.

**`conversation_messages`** (renamed from `messages`)

No more `role=tool`. No more `tool_calls` or `tool_result` JSONB columns. Roles retained:
`user`, `assistant`, `system`, `event`.

```sql
conversation_messages
─────────────────────
id                    UUID        PK, server-generated
conversation_id       UUID        FK → conversations ON DELETE CASCADE
user_id               UUID        FK → users ON DELETE SET NULL, nullable
role                  message_role  'user' | 'assistant' | 'system' | 'event'
content               TEXT        NOT NULL
reasoning             TEXT        nullable (LLM thinking/chain-of-thought)
token_count           INTEGER     nullable
metadata              JSONB       nullable (provider-specific, ad-hoc)
created_at            TIMESTAMPTZ NOT NULL DEFAULT now()
```

**`conversation_tool_executions`** (new)

Each tool call is a first-class row with explicit lifecycle tracking.

```sql
conversation_tool_executions
────────────────────────────
id                      UUID                  PK, server-generated
conversation_id         UUID                  FK → conversations ON DELETE CASCADE
conversation_message_id UUID                  FK → conversation_messages ON DELETE CASCADE
tool_call_id            TEXT                  NOT NULL (LLM-assigned string)
tool_name               TEXT                  NOT NULL
input                   JSONB                 NOT NULL
source                  tool_execution_source 'builtin' | 'plugin' | 'mcp'
status                  tool_execution_status 'pending' | 'running' | 'completed' | 'failed' | 'cancelled'
output                  TEXT                  nullable (null while pending)
error                   TEXT                  nullable
plugin_id               UUID                  FK → plugins ON DELETE SET NULL, nullable
created_at              TIMESTAMPTZ           NOT NULL DEFAULT now()
started_at              TIMESTAMPTZ           nullable
completed_at            TIMESTAMPTZ           nullable
```

**Key design decisions:**
- `conversation_message_id` FK explicitly links tool calls to the assistant message that
  requested them. No more string-matching `tool_call_id` across separate rows.
- `status` enables write-ahead: INSERT with `pending` before execution, UPDATE after.
- `source` tracks tool origin: `builtin` (shipped with agent), `plugin` (WASM), `mcp` (external).
- `plugin_id` links plugin/skill tool executions back to the plugin registry.
- All tool executions are persisted, including those from tools previously marked `internal`.
  The `internal` flag on `ToolMetadata` controls whether the execution is included in
  LLM context reconstruction — not whether it's persisted.
- Message UUIDs are generated server-side (`gen_random_uuid()`), not by the frontend.
- UNIQUE constraint on `(conversation_message_id, tool_call_id)` prevents duplicate insertions.

### LLM Message Reconstruction

One query loads conversation history for LLM calls:

```sql
SELECT m.id, m.role, m.content, m.reasoning, m.created_at,
       json_agg(json_build_object(
           'id', te.tool_call_id,
           'function', json_build_object('name', te.tool_name, 'arguments', te.input),
           'status', te.status,
           'result', te.output,
           'error', te.error
       ) ORDER BY te.created_at) FILTER (WHERE te.id IS NOT NULL) as tool_calls
FROM conversation_messages m
LEFT JOIN conversation_tool_executions te ON te.conversation_message_id = m.id
WHERE m.conversation_id = $1
GROUP BY m.id
ORDER BY m.created_at;
```

Rust conversion to OpenAI format: for each assistant row with tool_calls, emit:
1. An assistant `Message` with `tool_calls` array
2. N tool `Message` entries (one per tool execution with `completed`/`failed` status)
3. Skip `pending`/`running` entries (synthesize error result for these)

The old `domain_to_llm_messages` function and `sanitize_tool_call_pairs` are replaced by
this query + a simpler Rust conversion in `history.rs`. Orphan handling is unnecessary —
the FK constraint makes orphans structurally impossible.

---

## Write-Ahead Persistence

**Principle:** Nothing executes until its intent is persisted.

```
1. LLM returns assistant + tool_calls
2. Store assistant message in conversation_messages
3. For each tool_call:
   a. INSERT into conversation_tool_executions (status='pending')
   b. UPDATE status='running', started_at=now()
   c. Execute tool
   d. UPDATE status='completed'/'failed', output/error, completed_at=now()
   e. Add result to in-memory llm_messages for next LLM call
```

If step (a) fails → tool is NOT executed, error returned to LLM.
If step (d) fails → retry once. If still fails, row stays `running` — recovered on restart.
If process crashes after (a) but before (d) → rows remain `pending` or `running` in DB,
detected and marked `failed` on next actor spawn for this conversation.

---

## Error Handling & Recovery

### Tool execution fails (normal error)

```
DB:     status: pending → running → failed, error: "connection refused"
LLM:    sees tool result with error content, decides to retry or report
Actor:  continues to next tool_call in the batch
```

### Tool execution panics

Each tool execution is wrapped in panic-catching. One tool's panic does not prevent
others from executing.

```
DB:     status: running → failed, error: "tool panicked during execution"
Actor:  continues to next tool_call
```

### Agent process crashes mid-execution

On actor spawn, check for incomplete tool executions:

```sql
UPDATE conversation_tool_executions
SET status = 'failed', error = 'Agent restarted during execution', completed_at = now()
WHERE conversation_id = $1 AND status IN ('pending', 'running');
```

Additionally, persist an assistant message notifying the user:

> "Previous operation was interrupted. Some tool calls did not complete. You may need to
> retry your last request."

### User sends message during tool execution

Message waits in the actor's inbox channel. When the current turn finishes, the actor
picks up the next message with clean, fully-persisted state. No concurrent loops.

### DB write fails for write-ahead INSERT

Tool is NOT executed. Error returned to LLM as a tool failure result. No orphan possible.

### User feedback on failures

- Tool errors always include tool name and human-readable error in the execution row
- When the error circuit breaker trips (3 consecutive failures), the forced-text message
  includes which tools failed and why
- Frontend can show tool execution status (pending/running/completed/failed) in real-time
  via existing event stream

---

## Streaming LLM Responses

Switch from `stream: false` (buffered) to `stream: true` (SSE) for the main LLM
completion call. The plumbing is already built end-to-end:

- `LlmEngine::stream()` — implemented in OpenAI-compatible client and ACP transport
- `StreamChunk` type + SSE parser in `sober-llm/src/streaming.rs`
- `TextDelta` event in proto `ConversationUpdate` oneof
- `AgentEvent::TextDelta` variant in agent event stream
- Frontend already handles `TextDelta` via WebSocket

**Changes in `turn.rs`:**

Replace `llm.complete(req)` with `llm.stream(req)`. Process chunks in a loop:
- Text content deltas → send `TextDelta` events immediately to the user
- Tool call deltas → buffer incrementally (arguments arrive as string fragments)
- On stream end → assemble final text + complete tool calls, proceed to tool execution

`sober-llm` already provides `collect_stream()` which buffers a stream into a full
`CompletionResponse`. For the initial implementation, use a hybrid approach:
1. Forward `TextDelta` events to the user as chunks arrive (real-time text)
2. Use `collect_stream()` to assemble tool calls (buffer until complete)

Title generation stays non-streaming (short internal call, no user-facing latency).

---

## Repository Layer Changes

A new `ToolExecutionRepo` trait is added to `sober-core/src/types/repo.rs`:

```rust
pub trait ToolExecutionRepo: Send + Sync {
    fn create_pending(&self, ...) -> impl Future<Output = Result<ToolExecution, AppError>>;
    fn update_status(&self, id: ToolExecutionId, status, output, error) -> ...;
    fn find_incomplete(&self, conversation_id: ConversationId) -> ...; // pending/running
    fn find_by_message(&self, message_id: MessageId) -> ...;
}
```

Concrete `PgToolExecutionRepo` in `sober-db/src/repos/`.

The `AgentRepos` trait bundle in `sober-agent/src/agent_repos.rs` gains a new associated type:
```rust
type ToolExec: ToolExecutionRepo;
fn tool_executions(&self) -> &Self::ToolExec;
```

The existing `MessageRepo` is updated:
- `create()` no longer accepts `tool_calls` or `tool_result` fields
- Remove `role: Tool` from `CreateMessage` input
- Return server-generated UUID from `create()`

---

## Module Structure

Current `agent.rs` (2,088 lines) splits into focused modules:

```
backend/crates/sober-agent/src/
├── agent.rs              (~300 lines)  Agent struct, ActorRegistry, send_message(),
│                                       workspace resolution, accessors
├── conversation.rs       (~200 lines)  ConversationActor, inbox loop, idle timeout,
│                                       actor lifecycle (spawn/shutdown)
├── turn.rs               (~450 lines)  run_turn(): context load → prompt assembly
│                                       → LLM call → tool/text response handling
├── dispatch.rs           (~250 lines)  execute_tool_calls(), handle_confirmation(),
│                                       write-ahead persistence for tool executions
├── history.rs            (~200 lines)  DB → LLM message format conversion,
│                                       tool execution → OpenAI tool messages
├── ingestion.rs          (~90 lines)   Background memory extraction & embedding
│
├── confirm.rs            (unchanged)   Confirmation broker
├── extraction.rs         (unchanged)   Parse <memory_extractions> blocks
├── stream.rs             (unchanged)   AgentEvent enum
├── broadcast.rs          (unchanged)   Broadcast channel types
├── error.rs              (unchanged)   AgentError enum
├── backends.rs           (unchanged)   LLM engine abstraction
├── audit.rs              (unchanged)   Audit logging
├── system_jobs.rs        (unchanged)   Internal job executors
├── grpc/                 (unchanged)   gRPC handlers
├── tools/                (unchanged)   Tool implementations
└── main.rs               (unchanged)   Process entry point
```

| Module | Responsibility | Depends on |
|--------|---------------|------------|
| `agent.rs` | Actor registry, public API, workspace resolution | conversation.rs |
| `conversation.rs` | Inbox channel, message queuing, idle timeout | turn.rs |
| `turn.rs` | Single LLM turn: embed → context → prompt → complete → handle | dispatch.rs, history.rs |
| `dispatch.rs` | Tool execution loop, write-ahead DB, confirmation flow | tools/, confirm.rs |
| `history.rs` | `conversation_messages` + `conversation_tool_executions` → `Vec<LlmMessage>` | sober-llm, sober-core |
| `ingestion.rs` | Background memory extraction storage | sober-memory, sober-llm |

---

## Migration

Single migration, wrapped in a transaction. Four steps:

### Step 1: Create new table and enums

```sql
CREATE TYPE tool_execution_source AS ENUM ('builtin', 'plugin', 'mcp');
CREATE TYPE tool_execution_status AS ENUM ('pending', 'running', 'completed', 'failed', 'cancelled');

CREATE TABLE conversation_tool_executions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    conversation_message_id UUID NOT NULL,
    tool_call_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    input JSONB NOT NULL DEFAULT '{}',
    source tool_execution_source NOT NULL DEFAULT 'builtin',
    status tool_execution_status NOT NULL DEFAULT 'completed',
    output TEXT,
    error TEXT,
    plugin_id UUID REFERENCES plugins(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ
);
```

### Step 2: Migrate tool result data

```sql
-- Pre-flight: count orphaned tool rows that cannot be matched to an assistant message.
-- These are the exact rows that caused the production bugs — they will be dropped.
DO $$
DECLARE orphan_count INTEGER;
BEGIN
    SELECT COUNT(*) INTO orphan_count
    FROM messages t
    LEFT JOIN LATERAL (
        SELECT m.id FROM messages m
        WHERE m.conversation_id = t.conversation_id
        AND m.role = 'assistant' AND m.tool_calls IS NOT NULL
        AND m.tool_calls @> jsonb_build_array(
            jsonb_build_object('id', t.tool_result->>'tool_call_id'))
        ORDER BY m.created_at DESC LIMIT 1
    ) a ON true
    WHERE t.role = 'tool' AND t.tool_result IS NOT NULL AND a.id IS NULL;

    RAISE NOTICE 'Orphaned tool rows (will be dropped): %', orphan_count;
END $$;

-- Move matched role='tool' rows into conversation_tool_executions.
-- Orphaned tool rows (no matching assistant message) are intentionally skipped —
-- they represent the corrupted state this rewrite eliminates.
INSERT INTO conversation_tool_executions
    (conversation_id, conversation_message_id, tool_call_id, tool_name,
     input, status, output, created_at, completed_at)
SELECT
    t.conversation_id,
    a.id,
    t.tool_result->>'tool_call_id',
    t.tool_result->>'name',
    '{}',
    'completed',
    t.content,
    t.created_at,
    t.created_at
FROM messages t
JOIN LATERAL (
    SELECT m.id FROM messages m
    WHERE m.conversation_id = t.conversation_id
    AND m.role = 'assistant' AND m.tool_calls IS NOT NULL
    AND m.tool_calls @> jsonb_build_array(
        jsonb_build_object('id', t.tool_result->>'tool_call_id'))
    ORDER BY m.created_at DESC LIMIT 1
) a ON true
WHERE t.role = 'tool' AND t.tool_result IS NOT NULL;

-- Backfill tool arguments from assistant tool_calls JSONB.
-- Guard against malformed arguments with COALESCE and IS NOT NULL check.
UPDATE conversation_tool_executions te
SET input = COALESCE(
    (tc.elem->'function'->>'arguments')::jsonb,
    '{}'::jsonb
)
FROM messages m,
     jsonb_array_elements(m.tool_calls) AS tc(elem)
WHERE te.conversation_message_id = m.id
AND tc.elem->>'id' = te.tool_call_id
AND te.input = '{}'
AND tc.elem->'function'->>'arguments' IS NOT NULL;
```

### Step 3: Rename table, add columns, drop old columns

```sql
-- Rename
ALTER TABLE messages RENAME TO conversation_messages;

-- Add reasoning column
ALTER TABLE conversation_messages ADD COLUMN reasoning TEXT;

-- Backfill reasoning from metadata
UPDATE conversation_messages
SET reasoning = metadata->>'reasoning_content'
WHERE metadata ? 'reasoning_content';

-- Clean metadata
UPDATE conversation_messages
SET metadata = metadata - 'reasoning_content'
WHERE metadata ? 'reasoning_content';

-- Rename message_tags for consistency
ALTER TABLE message_tags RENAME TO conversation_message_tags;

-- Delete migrated tool rows.
-- Note: any conversation_message_tags attached to tool messages will cascade-delete.
-- This is acceptable — tool result tags are not meaningful after migration.
DELETE FROM conversation_messages WHERE role = 'tool';

-- Drop old columns
ALTER TABLE conversation_messages DROP COLUMN tool_calls;
ALTER TABLE conversation_messages DROP COLUMN tool_result;

-- Swap enum to remove 'tool' value
CREATE TYPE message_role_v2 AS ENUM ('system', 'user', 'assistant', 'event');
ALTER TABLE conversation_messages
    ALTER COLUMN role TYPE message_role_v2 USING role::text::message_role_v2;
DROP TYPE message_role;
ALTER TYPE message_role_v2 RENAME TO message_role;
```

### Step 4: Add FK and indexes

```sql
ALTER TABLE conversation_tool_executions
ADD CONSTRAINT fk_tool_exec_message
FOREIGN KEY (conversation_message_id)
REFERENCES conversation_messages(id) ON DELETE CASCADE;

CREATE INDEX idx_tool_exec_conversation
    ON conversation_tool_executions(conversation_id);
CREATE INDEX idx_tool_exec_message
    ON conversation_tool_executions(conversation_message_id);
CREATE INDEX idx_tool_exec_pending
    ON conversation_tool_executions(conversation_id, status)
    WHERE status IN ('pending', 'running');

ALTER TABLE conversation_tool_executions
ADD CONSTRAINT uq_tool_exec_message_call
UNIQUE (conversation_message_id, tool_call_id);
```

---

## Frontend Impact

### Message Loading — Tool Executions Inline on Assistant Messages

The `GET /api/v1/conversations/:id/messages` response changes. Assistant messages that
triggered tool calls include a `tool_executions` array:

```json
{
  "data": [
    {
      "id": "msg-123",
      "role": "user",
      "content": "Search for latest news"
    },
    {
      "id": "msg-456",
      "role": "assistant",
      "content": "Let me search for that.",
      "tool_executions": [
        {
          "id": "exec-1",
          "tool_call_id": "tool_abc",
          "tool_name": "web_search",
          "input": {"query": "latest news"},
          "status": "completed",
          "output": "1. Reuters...",
          "error": null,
          "source": "builtin",
          "started_at": "...",
          "completed_at": "..."
        }
      ]
    }
  ]
}
```

No more `role: "tool"` messages in the response. The frontend renders tool results as
part of the assistant message's UI, not as standalone message cards.

### Message Creation — Two-Phase with Server ID

```
Frontend: POST /api/v1/conversations/:id/messages { content: "hello" }
Backend:  INSERT → returns { data: { id: "server-uuid", ... } }
Frontend: uses server-generated ID for all subsequent references
```

The frontend drops its UUID generation for messages. The WebSocket subscription uses
server IDs for event routing.

### Live Updates — New `ToolExecutionUpdate` Event

Replace `ToolCallStart` + `ToolCallResult` with a single event type:

```json
{
  "event": "tool_execution_update",
  "conversation_id": "conv-123",
  "message_id": "msg-456",
  "execution": {
    "id": "exec-1",
    "tool_call_id": "tool_abc",
    "tool_name": "web_search",
    "status": "running",
    "output": null
  }
}
```

The frontend receives this for each status transition (`pending` → `running` →
`completed`/`failed`). It updates the tool execution card inline on the assistant message.

### Proto Changes

Update `ConversationUpdate.event` oneof:
- Remove `ToolCallStart`, `ToolCallResult` message types
- Add `ToolExecutionUpdate` message type with `id`, `message_id`, `tool_call_id`,
  `tool_name`, `status`, `output`, `error`

### Frontend TypeScript Changes

- Remove `ToolMessage` type (no more `role: "tool"`)
- Add `ToolExecution` type on `AssistantMessage`
- Update `ServerWsMessage` discriminated union: remove `ToolCallStart`/`ToolCallResult`,
  add `ToolExecutionUpdate`
- Remove client-side UUID generation for messages

---

## Verification

1. **Unit tests:** `history.rs` conversion, `dispatch.rs` write-ahead logic, `conversation.rs` inbox ordering
2. **Integration tests:** Full turn with tool calls against test DB — verify `conversation_tool_executions` rows
3. **Migration test:** Run migration on a copy of the prod DB (already available locally)
4. **Regression test:** Load the faulty conversation (`8f215099-e48f-4b4f-a664-cdac0d6145f9`) and verify the agent can process it without HTTP 400
5. **Concurrency test:** Send rapid messages during slow tool execution — verify sequential processing, no orphans
6. **Crash recovery test:** Kill agent mid-tool-execution, restart, verify pending rows are marked failed and user is notified
