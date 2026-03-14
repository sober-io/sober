# SubscribeConversationUpdates — Unified Event Delivery

## Context

Message delivery from the agent to the frontend is broken for scheduler-triggered jobs. Currently, event delivery is caller-coupled: whoever calls the agent's gRPC RPC gets the response stream. When the API calls `HandleMessage`, events flow to the WebSocket. When the scheduler calls `ExecuteTask`, events flow back to the scheduler — the API never sees them.

This redesign decouples event delivery from the caller by introducing a `SubscribeConversationUpdates` server-streaming RPC. The API subscribes once on startup and receives ALL conversation events regardless of who triggered the work.

## Design

### New RPC: `SubscribeConversationUpdates`

```
API ──SubscribeConversationUpdates──▶ Agent
                                        │
     ◀── stream of ConversationUpdate ──┘
```

- `HandleMessage` becomes **unary** (returns ack, no streaming)
- Agent publishes all events to a `tokio::sync::broadcast` channel
- `SubscribeConversationUpdates` subscribes to the broadcast and streams to the caller
- API routes events to the correct WebSocket(s) by `conversation_id`
- Fire-and-forget: if nobody is subscribed, events are dropped. Messages are in the DB.

### Proto Changes

**File:** `backend/proto/sober/agent/v1/agent.proto`

```protobuf
// HandleMessage becomes unary
rpc HandleMessage(HandleMessageRequest) returns (HandleMessageResponse);

message HandleMessageResponse {
  string message_id = 1;  // stored user message ID
}

// New subscription RPC
rpc SubscribeConversationUpdates(SubscribeRequest) returns (stream ConversationUpdate);

message SubscribeRequest {}

message ConversationUpdate {
  string conversation_id = 1;
  oneof event {
    NewMessage new_message = 2;
    TitleChanged title_changed = 3;
    TextDelta text_delta = 4;
    ToolCallStart tool_call_start = 5;
    ToolCallResult tool_call_result = 6;
    ThinkingDelta thinking_delta = 7;
    ConfirmRequest confirm_request = 8;
    Done done = 9;
    Error error = 10;
  }
}

message NewMessage {
  string message_id = 1;
  string role = 2;
  string content = 3;
}

message TitleChanged {
  string title = 1;
}
```

Existing `TextDelta`, `ToolCallStart`, `ToolCallResult`, `ThinkingDelta`, `ConfirmRequest`, `Done`, `Error` message definitions stay as-is — they're reused inside the `ConversationUpdate` oneof.

## Changes

### 1. Proto: new RPC and messages

**File:** `backend/proto/sober/agent/v1/agent.proto`

- Change `HandleMessage` return type from `stream AgentEvent` to `HandleMessageResponse`
- Add `HandleMessageResponse` message
- Add `SubscribeConversationUpdates` RPC
- Add `SubscribeRequest`, `ConversationUpdate`, `NewMessage`, `TitleChanged` messages

### 2. Agent: add broadcast channel

**File:** `backend/crates/sober-agent/src/broadcast.rs` (new)

- Define `ConversationUpdateSender` type alias for `broadcast::Sender<proto::ConversationUpdate>`
- Helper function to create the channel

**File:** `backend/crates/sober-agent/src/main.rs`

- Create broadcast channel
- Pass sender to `Agent` and gRPC service

### 3. Agent: publish events to broadcast

**File:** `backend/crates/sober-agent/src/agent.rs`

- Add `broadcast_tx: ConversationUpdateSender` field to `Agent`
- In `run_loop_streaming`: after sending events to `event_tx`, also publish to `broadcast_tx` with `conversation_id` wrapped in `ConversationUpdate`
- After storing assistant message: publish `NewMessage` event
- After generating title: publish `TitleChanged` event

### 4. Agent: implement SubscribeConversationUpdates

**File:** `backend/crates/sober-agent/src/grpc.rs`

- Change `HandleMessage` to unary: store user message, spawn agentic loop, return `HandleMessageResponse` with message ID immediately
- Add `SubscribeConversationUpdates` implementation: subscribe to broadcast, forward events through gRPC response stream
- Add `SubscribeConversationUpdatesStream` type alias

### 5. API: add conversation connection registry

**File:** `backend/crates/sober-api/src/connections.rs` (new)

- `ConnectionRegistry` struct: `Arc<RwLock<HashMap<String, Vec<mpsc::Sender<ServerWsMessage>>>>>`
- `register(conversation_id, sender)` — adds a sender
- `unregister(conversation_id, sender)` — removes a specific sender
- `send(conversation_id, message)` — sends to all registered senders for that conversation, removes dead senders

### 6. API: subscribe to agent on startup

**File:** `backend/crates/sober-api/src/state.rs`

- Add `ConnectionRegistry` to `AppState`

**File:** `backend/crates/sober-api/src/subscribe.rs` (new)

- `spawn_subscription(agent_client, registry)` — background task that:
  - Calls `SubscribeConversationUpdates` on the agent
  - Receives `ConversationUpdate` events
  - Converts to `ServerWsMessage`
  - Routes via `ConnectionRegistry::send()`
  - Reconnects with backoff on disconnect

**File:** `backend/crates/sober-api/src/main.rs` (or wherever the server starts)

- Spawn subscription task after `AppState` is created

### 7. API: update WebSocket handler

**File:** `backend/crates/sober-api/src/routes/ws.rs`

- Remove `handle_chat_message` function (no more direct stream consumption)
- On `ChatMessage`: call unary `HandleMessage` RPC (fire-and-forget), register sender in `ConnectionRegistry`
- On disconnect: unregister all senders for this connection
- Remove `active_tasks` tracking — cancellation handled by unregistering from registry
- Add `ChatNewMessage` to `ServerWsMessage` enum

### 8. Frontend: handle new message type

**File:** `frontend/src/lib/types/index.ts`

- Add `chat.new_message` variant to `ServerWsMessage` type

**File:** `frontend/src/routes/(app)/chat/[id]/+page.svelte`

- Handle `chat.new_message` — append message to conversation list

## Files Summary

| File | Change |
|------|--------|
| `proto/sober/agent/v1/agent.proto` | Unary HandleMessage, new SubscribeConversationUpdates RPC |
| `sober-agent/src/broadcast.rs` | **New** — broadcast channel type + constructor |
| `sober-agent/src/agent.rs` | Add broadcast_tx, publish events |
| `sober-agent/src/grpc.rs` | Unary HandleMessage, SubscribeConversationUpdates impl |
| `sober-agent/src/main.rs` | Create broadcast, wire through |
| `sober-agent/src/stream.rs` | No change (existing AgentEvent stays for internal use) |
| `sober-api/src/connections.rs` | **New** — ConnectionRegistry |
| `sober-api/src/subscribe.rs` | **New** — subscription task |
| `sober-api/src/state.rs` | Add ConnectionRegistry to AppState |
| `sober-api/src/routes/ws.rs` | Rewrite to use registry, unary HandleMessage |
| `sober-api/src/main.rs` | Spawn subscription task |
| `frontend/src/lib/types/index.ts` | Add chat.new_message type |
| `frontend/src/routes/(app)/chat/[id]/+page.svelte` | Handle new_message event |

## Verification

```bash
# Backend builds
cd backend
cargo build -q -p sober-agent -p sober-api
cargo clippy -q -p sober-agent -p sober-api -- -D warnings
cargo test -p sober-agent -p sober-api -q

# Frontend builds
cd frontend
pnpm check
pnpm build --silent

# Integration test
# 1. Start containers: docker compose up -d
# 2. Open WS in browser, send a message — should get events via subscription
# 3. Create a scheduled job with conversation_id — verify events arrive in real-time
# 4. Open same conversation in two browser tabs — both should receive events
# 5. Kill and restart API — should auto-reconnect subscription
```
