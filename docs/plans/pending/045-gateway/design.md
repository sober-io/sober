# #045: sober-gateway — External Messaging Platform Bridge

## Overview

`sober-gateway` is an internal service that bridges external messaging platforms
(Discord, Telegram, Matrix, WhatsApp) to Sõber. It translates platform events
into Sõber messages and delivers Sõber responses back to the originating platform.

Single binary, multi-platform via cargo feature flags. Fifth internal process
alongside web, api, agent, scheduler.

```
Discord ──┐
Telegram ──┤                    ┌───────────────┐
Matrix  ───┼── Platform SDKs ──▶│ sober-gateway │──gRPC/UDS──▶ sober-agent
WhatsApp ──┘                    │               │──SQL───────▶ PostgreSQL
                                └───────────────┘
```

## Architecture

### Communication

- **gRPC/UDS** to `sober-agent` for `HandleMessage` + `SubscribeConversationUpdates`
  (same pattern as `sober-scheduler`)
- **Direct DB access** via `sober-db` for conversation CRUD, mapping tables, config
- **Admin socket** (UDS) at `/run/sober/gateway.sock` for runtime control

### Crate Dependencies

```
sober-gateway → sober-core   (types, errors)
              → sober-db     (repos, pool)
              → sober-agent  (gRPC client — proto only)
```

No crate depends on `sober-gateway`. It is a leaf binary like `sober-api`.

### Event Delivery

The gateway subscribes to `SubscribeConversationUpdates` — the same broadcast
stream that `sober-api` subscribes to. All conversation events (from any source:
web UI, scheduler, or platform users) flow through the agent's broadcast channel
to all subscribers.

```
Any message source
    → agent stores + broadcasts ConversationUpdate
         │
         ├──▶ sober-api ──▶ WebSocket ──▶ frontend
         └──▶ sober-gateway ──▶ Discord/Telegram/etc.
```

A message sent from Discord appears in the web UI. A message sent from the web UI
appears in Discord. A scheduled job response appears in both.

## Conversation Mapping

### Channel ↔ Conversation (1:1)

Each external channel, group, or room maps to exactly one Sõber conversation.

| Platform | Maps to conversation |
|----------|---------------------|
| Discord  | Channel             |
| Telegram | Group / supergroup  |
| Matrix   | Room                |
| WhatsApp | Group               |
| DMs (any)| DM conversation     |

### Threads → New Independent Conversation

When a thread is created in a mapped channel, the gateway creates a new,
independent Sõber conversation. No parent-child link in the conversation model.
The mapping table tracks the thread→channel relationship for cleanup purposes
only (`parent_mapping_id`).

### Auto-Create on First Interaction

Default behavior: the bot sits idle in a channel until someone @mentions it or
sends a DM. On first interaction, the gateway auto-creates a Sõber conversation
and stores the mapping. No upfront configuration needed beyond adding the bot token.

Manual mapping is also supported: users can pre-configure channel→conversation
mappings via the web UI, including mapping to existing conversations.

### Agent Mode Defaults

| Context | Default mode |
|---------|-------------|
| DMs     | `Always` — respond to every message |
| Groups  | `Mention` — respond only when @mentioned |

Configurable per mapping.

## User Identity Mapping

Hybrid model:

- **Mapped users**: Admin links external user → Sõber user in the web UI.
  Messages from mapped users are attributed to their Sõber account (proper
  `user_id`, memory scoping, permissions).
- **Unmapped users**: Messages go through a bridge service account. The external
  username is prefixed in the message content: `[harri] hey can you check the logs?`.

This keeps the barrier low (works immediately without mapping every user) while
allowing proper attribution for key users.

## Data Model

### `gateway_platforms`

Registered platform connections (one row per bot/token).

| Column | Type | Notes |
|--------|------|-------|
| `id` | `uuid` (PK) | `PlatformId` |
| `platform_type` | `text` | `discord`, `telegram`, `matrix`, `whatsapp` |
| `display_name` | `text` | User-friendly label |
| `credentials` | `jsonb` | Encrypted via `sober-crypto` envelope encryption |
| `is_enabled` | `bool` | Toggle without deleting |
| `created_at` | `timestamptz` | |
| `updated_at` | `timestamptz` | |

### `gateway_channel_mappings`

Maps external channels to Sõber conversations.

| Column | Type | Notes |
|--------|------|-------|
| `id` | `uuid` (PK) | `MappingId` |
| `platform_id` | `uuid` (FK) | → `gateway_platforms` |
| `external_channel_id` | `text` | Platform's channel/group/room ID |
| `external_channel_name` | `text` | Display name (synced from platform) |
| `conversation_id` | `uuid` (FK) | → `conversations` |
| `agent_mode` | `text` | `always`, `mention`, `silent` |
| `is_thread` | `bool` | Whether this maps a thread |
| `parent_mapping_id` | `uuid` (FK, nullable) | → self, for thread→channel cleanup |
| `created_at` | `timestamptz` | |

Unique constraint: `(platform_id, external_channel_id)`

### `gateway_user_mappings`

Maps external users to Sõber users.

| Column | Type | Notes |
|--------|------|-------|
| `id` | `uuid` (PK) | |
| `platform_id` | `uuid` (FK) | → `gateway_platforms` |
| `external_user_id` | `text` | Platform's user ID |
| `external_username` | `text` | Display name (synced) |
| `user_id` | `uuid` (FK) | → `users` |
| `created_at` | `timestamptz` | |

Unique constraint: `(platform_id, external_user_id)`

## Platform Trait

```rust
pub trait PlatformBridge: Send + Sync + 'static {
    /// Connect to the platform and start receiving events.
    async fn connect(&mut self, config: PlatformConfig) -> Result<(), GatewayError>;

    /// Disconnect gracefully.
    async fn disconnect(&mut self) -> Result<(), GatewayError>;

    /// Send a message to an external channel.
    async fn send_message(&self, channel_id: &str, content: PlatformMessage) -> Result<(), GatewayError>;

    /// Platform identifier.
    fn platform_type(&self) -> PlatformType;

    /// Resolve display name for a channel ID.
    async fn resolve_channel_name(&self, channel_id: &str) -> Result<String, GatewayError>;

    /// List channels the bot has access to (for manual mapping UI).
    async fn list_channels(&self) -> Result<Vec<ExternalChannel>, GatewayError>;
}
```

Implementations emit events into an internal `tokio::mpsc` channel. They do not
call Sõber APIs directly.

### Gateway Events

```rust
pub enum GatewayEvent {
    MessageReceived {
        platform_id: PlatformId,
        channel_id: String,
        user_id: String,
        username: String,
        content: String,
    },
    ThreadCreated {
        platform_id: PlatformId,
        parent_channel_id: String,
        thread_id: String,
        thread_name: String,
    },
    ReactionAdded { ... },
    ChannelDeleted { ... },
}
```

### Outbound Message Type

```rust
pub struct PlatformMessage {
    pub text: String,
    pub format: MessageFormat, // Markdown, Plain
    pub attachments: Vec<Attachment>,
    pub reply_to: Option<String>, // External message ID to reply to
}
```

Each `PlatformBridge` implementation converts `PlatformMessage` into
platform-specific format. Inbound messages are normalized to plain text/markdown.

## Gateway Core

### Inbound Flow (Platform → Sõber)

1. Platform message arrives (e.g., Discord #dev)
2. `PlatformBridge` emits `GatewayEvent::MessageReceived`
3. Gateway core receives event
4. Look up mapping: `(platform_id, channel_id)` → `ConversationId`
   - Found → use it
   - Not found → create conversation, insert mapping, cache it
5. Look up user: `(platform_id, external_user_id)` → `UserId`
   - Found → use mapped `user_id`
   - Not found → use bridge service account, prefix content with `[username]`
6. Call `agent.HandleMessage(user_id, conversation_id, content)`

### Outbound Flow (Sõber → Platform)

1. `ConversationUpdate` arrives via `SubscribeConversationUpdates`
2. Look up reverse mapping: `conversation_id` → `Vec<(platform_id, channel_id)>`
3. Buffer `TextDelta` fragments (don't send token-by-token)
4. On `Done` event → flush buffered content via `bridge.send_message()`

**Duplicate prevention:** Skip forwarding `NewMessage` events when the `user_id`
matches a gateway-mapped user and role is `User` — prevents echoing inbound
messages back to the platform they came from.

### Response Buffering Strategy

Platforms have rate limits and different editing capabilities:

| Platform | Strategy |
|----------|----------|
| Discord  | Send initial message on first delta, edit every ~1s with accumulated text |
| Telegram | Same edit-in-place via `editMessageText` |
| Matrix   | Message replacement (edit-in-place) |
| WhatsApp | No editing — buffer entire response, send once on `Done` |

### Startup Sequence

1. Load platform configs from DB (`gateway_platforms`)
2. Connect to `sober-agent` gRPC/UDS
3. Subscribe to `SubscribeConversationUpdates` (with reconnect + backoff)
4. For each enabled platform: spawn `PlatformBridge`, call `connect()`
5. Load all `gateway_channel_mappings` into in-memory `DashMap`
6. Start `GatewayEvent` processing loop
7. Open admin socket at `/run/sober/gateway.sock`

### Admin Socket Commands

- `status` — list connected platforms, active mappings count
- `reload` — re-read platform configs from DB, connect/disconnect as needed
- `disconnect <platform_id>` — graceful disconnect of one platform

## API Endpoints

All under `/api/v1/admin/gateway/`, admin-only.

### Platform Management

- `GET /platforms` — list configured platforms
- `POST /platforms` — add platform (type, credentials, display name)
- `PATCH /platforms/:id` — update credentials, enable/disable
- `DELETE /platforms/:id` — remove platform + all its mappings

### Channel Mapping Management

- `GET /platforms/:id/channels` — list available channels from platform (proxied
  to gateway via admin socket; requires gateway to be running and platform connected)
- `GET /platforms/:id/mappings` — list existing mappings
- `POST /platforms/:id/mappings` — create manual mapping
- `DELETE /mappings/:id` — remove mapping (keeps conversation)

### User Mapping Management

- `GET /platforms/:id/users` — list user mappings
- `POST /platforms/:id/users` — map external user → Sõber user
- `DELETE /user-mappings/:id` — remove user mapping

## Web UI

Settings pages:

- **Gateway** — list platforms, add/remove, toggle enabled
- **Platform detail** — credentials (masked), channel mappings, user mappings
- **Map channel** — select from platform's channel list, pick or create conversation, set agent mode

## Observability

### Metrics

| Metric | Type | Labels |
|--------|------|--------|
| `gateway_messages_received_total` | counter | `platform`, `channel_id` |
| `gateway_messages_sent_total` | counter | `platform`, `channel_id` |
| `gateway_message_handle_duration_seconds` | histogram | `platform` |
| `gateway_message_delivery_duration_seconds` | histogram | `platform` |
| `gateway_platform_connections` | gauge | `platform`, `status` |
| `gateway_mappings_auto_created_total` | counter | `platform` |
| `gateway_buffer_flush_size_bytes` | histogram | `platform` |
| `gateway_platform_errors_total` | counter | `platform`, `error_type` |

### Dashboard Panels

- Connection status per platform (uptime, state)
- Message throughput (inbound/outbound rate)
- Latency p50/p95/p99 (handle + delivery)
- Error rate by platform and type
- Active mapping count per platform

### Logging

Structured tracing:

- `INFO` — platform connect/disconnect, mapping created/removed
- `WARN` — rate limit hit, reconnecting, unmapped user fallback
- `ERROR` — auth failure, gRPC connection lost, delivery failed
- `DEBUG` — individual message routing, buffer flushes

## Deployment

- **Docker**: `infra/docker/Dockerfile.gateway`, added to `docker-compose.yml` and `docker-bake.hcl`
- **Systemd**: `sober-gateway.service` unit file
- **Install script**: updated to include gateway binary + service
- **Admin socket**: `/run/sober/gateway.sock`

## Credential Storage

Credentials are encrypted JSONB, per-platform shape:

```json
// Discord
{ "bot_token": "encrypted:..." }

// Telegram
{ "bot_token": "encrypted:..." }

// Matrix
{ "homeserver_url": "https://...", "access_token": "encrypted:..." }

// WhatsApp
{ "phone_number_id": "...", "access_token": "encrypted:..." }
```

Encryption via `sober-crypto` envelope encryption (same as MCP server credentials).

## Scope

### This Plan

- Gateway core: event loop, mapping logic, gRPC client, auto-create, buffering
- Platform trait: `PlatformBridge`, `GatewayEvent`, `PlatformMessage`
- Discord implementation: channels, threads, message editing, @mention detection
- Data model: 3 tables + migrations
- API endpoints: platform/mapping/user CRUD
- Web UI: gateway settings pages
- Metrics, dashboard, service files, docs

### Future Platforms (separate plans)

- Telegram
- Matrix
- WhatsApp

Each is a self-contained `PlatformBridge` implementation + cargo feature.

### Future Enhancements (not in scope)

- Rich content (embeds, buttons, reactions)
- File/image attachments via blob store
- Per-channel tool restrictions
- Platform-specific commands (Discord slash commands)
- Typing indicators
