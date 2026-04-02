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
- **gRPC/UDS** server at `/run/sober/gateway.sock` — exposes `ListChannels` RPC
  so `sober-api` can proxy platform channel listings to the web UI
- **DB access** via `sober-db` repos for mapping tables, user lookups, config

### Crate Dependencies

```
sober-gateway → sober-core   (types, errors)
              → sober-db     (repos, pool)
              → sober-crypto (credential decryption)
              → sober-agent  (gRPC client — proto only)
```

No crate depends on `sober-gateway`. It is a leaf binary like `sober-scheduler`.
It has no HTTP server — platform/mapping CRUD endpoints live in `sober-api`
(see API Endpoints section).

### Service Layer

The gateway binary extracts business logic into a service struct. The event
loop remains thin — receives events, calls service, dispatches results.

```rust
pub struct GatewayService {
    db: PgPool,
    agent: AgentClient,
    bridges: Arc<PlatformBridgeRegistry>,
}
```

`GatewayService` owns:
- **Inbound routing** — mapping lookup, user attribution, message forwarding
- **Outbound delivery** — buffer management, platform dispatch

Methods return `Result<T, AppError>` with typed response DTOs.

Platform/mapping/user CRUD is handled by a separate `GatewayAdminService` in
`sober-api` (see API Endpoints section), not in this binary.

### Authorization

The gateway binary is a headless event processor with no HTTP server. It
authenticates to `sober-agent` via UDS filesystem permissions (same pattern
as `sober-scheduler`).

Admin API routes for platform/mapping/user CRUD live in `sober-api` and use
the existing two-layer auth:

1. **Route-level**: `RequireAdmin` extractor on all `/admin/gateway/*` routes
2. **Service-level**: `guards::require_admin()` calls where needed

### Bridge Bot User

A dedicated bot user is seeded in migrations with a service/bot role. All
platform messages from unmapped users use this bot's `user_id` in
`HandleMessage`. The bot user provides a real identity for conversation
membership and memory scoping.

**Membership requirement:** When a channel mapping is created, the
`GatewayAdminService` must add the bridge bot user as a member of the
target conversation (via `_tx` within the mapping creation transaction).
The agent does not verify membership on `HandleMessage` — the gateway
and admin service are responsible for ensuring it exists.

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

### Threads

Thread support is deferred. Messages in threads of a mapped channel are routed
to the parent channel's conversation. The `is_thread` and `parent_mapping_id`
columns on `gateway_channel_mappings` exist for future per-thread conversation
mapping.

### Pre-Configured Mappings Only

Conversations must be created in Sõber first (via web UI or API), then mapped
to an external channel. The gateway does not auto-create conversations.

Admins configure mappings via the web UI: select a platform channel, pick an
existing Sõber conversation. The gateway picks up new
mappings on its next DB poll or gRPC `Reload` call.

### Agent Mode

Agent mode (`always`, `mention`, `silent`) is a property of the Sõber
conversation, not the gateway mapping. The agent decides whether to respond
based on the conversation's `agent_mode` (enforced in `sober-agent`).

When creating a conversation for gateway use, the admin sets the appropriate
agent mode on the conversation itself. The gateway does not override or
duplicate this setting.

## User Identity Mapping

Hybrid model:

- **Mapped users**: Admin links external user → Sõber user in the web UI.
  Messages from mapped users are attributed to their Sõber account (proper
  `user_id`, memory scoping, permissions).
- **Unmapped users**: Messages go through the bridge bot user (seeded in
  migrations). The external username is prefixed in the message content:
  `[harri] hey can you check the logs?`.

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
    ChannelDeleted {
        platform_id: PlatformId,
        channel_id: String,
    },
}
```

Additional variants (`ThreadCreated`, `ReactionAdded`, etc.) are added when
those features are implemented.

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
   - Not found → drop message, log warning (unmapped channel)
5. Look up user: `(platform_id, external_user_id)` → `UserId`
   - Found → use mapped `user_id`
   - Not found → use bridge bot user, prefix content with `[username]`
6. Call `agent.HandleMessage(user_id, conversation_id, content)`

#### ChannelDeleted Handler

When a platform reports a channel deletion, the gateway removes the mapping
from DB and the in-memory `DashMap`. Logged at INFO level. The conversation
is preserved — only the mapping is removed. Admins can re-create the mapping
if the deletion was a false alarm.

### Outbound Flow (Sõber → Platform)

1. `ConversationUpdate` arrives via `SubscribeConversationUpdates`
2. Look up reverse mapping: `conversation_id` → `Vec<(platform_id, channel_id)>`
3. Buffer `TextDelta` fragments (don't send token-by-token)
4. On `Done` event → flush buffered content via `bridge.send_message()`

**No echo filtering needed:** The agent only broadcasts `NewMessage` for
assistant responses — user messages are not broadcast. All assistant responses
should be forwarded to mapped platforms (cross-channel visibility is the
intended behavior).

**Unmapped conversations skipped:** If the reverse mapping lookup returns no
entries (conversation has no gateway mapping), the event is discarded. The
in-memory `DashMap` makes this an O(1) check per event.

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
3. Subscribe to `SubscribeConversationUpdates` (with reconnect + exponential backoff)
4. For each enabled platform: spawn `PlatformBridge`, call `connect()`
5. Load all `gateway_channel_mappings` + `gateway_user_mappings` into in-memory `DashMap`
6. Start `GatewayEvent` processing loop
7. Start gRPC server at `/run/sober/gateway.sock`

### gRPC Service

The gateway exposes its own gRPC service at `/run/sober/gateway.sock`:

- `ListChannels(platform_id)` — returns available channels from a connected
  platform. Called by `sober-api` to serve `GET /platforms/:id/channels`.
- `Reload()` — re-read platform configs from DB, connect/disconnect as needed.
- `Status()` — list connected platforms, active mappings count.
- `Health()` — liveness check.

Proto: `sober.gateway.v1.GatewayService`

## API Endpoints

All routes live in `sober-api` (not in the gateway binary) under
`/api/v1/admin/gateway/`, gated by `RequireAdmin` extractor.
Handlers delegate to a `GatewayAdminService` that reads/writes
gateway DB tables directly. The gateway binary picks up config
changes via gRPC `Reload` or periodic DB polling.

sober-api's `GatewayClient` is optional — the gateway is not required for
sober-api to start. All gateway admin endpoints except `GET /channels`
are DB-backed and work independently of the gateway process.

### Platform Management

- `GET /platforms` — list configured platforms
- `POST /platforms` — add platform (type, credentials, display name)
- `PATCH /platforms/:id` — update credentials, enable/disable
- `DELETE /platforms/:id` — remove platform + all its mappings

### Channel Mapping Management

- `GET /platforms/:id/channels` — list available channels from platform (calls
  gateway via gRPC; returns 503 if gateway is unavailable)
- `GET /platforms/:id/mappings` — list existing mappings
- `POST /platforms/:id/mappings` — create manual mapping
- `DELETE /mappings/:id` — remove mapping (keeps conversation)

### User Mapping Management

- `GET /platforms/:id/users` — list user mappings
- `POST /platforms/:id/users` — map external user → Sõber user. Also adds
  the Sõber user as a member to all conversations currently mapped for this
  platform (eager, via `_tx` transaction).
- `DELETE /user-mappings/:id` — remove user mapping

## Web UI

Settings pages:

- **Gateway** — list platforms, add/remove, toggle enabled
- **Platform detail** — credentials (masked), channel mappings, user mappings
- **Map channel** — select from platform's channel list, pick existing conversation

## Observability

### Metrics

| Metric | Type | Labels | Location | Description |
|--------|------|--------|----------|-------------|
| `sober_gateway_messages_received_total` | counter | `platform`, `status` | `GatewayService::handle_event` | Inbound platform messages (success/error) |
| `sober_gateway_messages_sent_total` | counter | `platform`, `status` | `GatewayService::deliver_outbound` | Outbound messages to platforms |
| `sober_gateway_message_handle_duration_seconds` | histogram | `platform` | `GatewayService::handle_event` | Inbound message processing latency |
| `sober_gateway_message_delivery_duration_seconds` | histogram | `platform` | `GatewayService::deliver_outbound` | Outbound delivery latency |
| `sober_gateway_platform_connections` | gauge | `platform`, `status` | `PlatformBridgeRegistry` | Active platform connections (connected/reconnecting/disconnected) |
| `sober_gateway_unmapped_messages_total` | counter | `platform` | `GatewayService::handle_event` | Messages dropped due to unmapped channel |
| `sober_gateway_buffer_flush_size_bytes` | histogram | `platform` | `GatewayService::deliver_outbound` | Size of buffered content flushed to platforms |
| `sober_gateway_platform_errors_total` | counter | `platform`, `error_type` | various | Platform SDK errors (auth, rate_limit, network, api) |

Cardinality: `platform` is bounded (discord/telegram/matrix/whatsapp). `status`
is bounded (success/error or connected/reconnecting/disconnected). No `channel_id`
labels — use logs for per-channel debugging.

### Trace Spans

| Span Name | Kind | Attributes | Context Propagation |
|-----------|------|------------|-------------------|
| `gateway.handle_event` | server | `platform`, `event_type`, `channel_id` | New root span per event |
| `gateway.resolve_mapping` | client | `platform`, `channel_id`, `matched` | Child of handle_event |
| `gateway.deliver_outbound` | client | `platform`, `conversation_id` | Extract from gRPC subscription metadata |
| `gateway.platform_connect` | client | `platform` | New root span |

Service methods use `#[instrument]` with `skip(self)` and relevant field bindings,
following the pattern established in `sober-api` services.

### metrics.toml Updates

New file: `backend/crates/sober-gateway/metrics.toml` — declares all metrics
listed above with alerts:

- `GatewayHighErrorRate`: `rate(sober_gateway_platform_errors_total[5m]) > 5` for 5m (warning)
- `GatewayPlatformDisconnected`: `sober_gateway_platform_connections{status="disconnected"} > 0` for 5m (warning)
- `GatewayHighP95Latency`: `histogram_quantile(0.95, rate(sober_gateway_message_handle_duration_seconds_bucket[5m])) > 2.0` for 5m (warning)

### Dashboard

New row: "Messaging Gateway" on the overview dashboard.

| Panel | Type | PromQL |
|-------|------|--------|
| Platform connections | stat | `sober_gateway_platform_connections` |
| Message throughput | timeseries | `rate(sober_gateway_messages_received_total[5m])`, `rate(sober_gateway_messages_sent_total[5m])` |
| Handle latency p50/p95/p99 | timeseries | `histogram_quantile(0.5\|0.95\|0.99, rate(sober_gateway_message_handle_duration_seconds_bucket[5m]))` |
| Error rate | timeseries | `rate(sober_gateway_platform_errors_total[5m])` by `platform`, `error_type` |
| Unmapped messages | timeseries | `rate(sober_gateway_unmapped_messages_total[5m])` by `platform` |

### Logging

Structured tracing:

- `INFO` — platform connect/disconnect, mapping created/removed
- `WARN` — rate limit hit, reconnecting, unmapped user fallback
- `ERROR` — auth failure, gRPC connection lost, delivery failed
- `DEBUG` — individual message routing, buffer flushes

## Deployment

- **Docker**: `infra/docker/Dockerfile.gateway`, added to `docker-compose.yml` and `docker-bake.hcl`
- **CI**: new target in `docker-bake.hcl`, binary added to `Dockerfile.ci` multi-stage build
- **Systemd**: `sober-gateway.service` unit file
- **Install script**: updated to include gateway binary + service
- **gRPC socket**: `/run/sober/gateway.sock`
- **Metrics**: `/metrics` endpoint for Prometheus scraping (same pattern as other services)

## Credential Storage

Platform credentials are stored in the existing `secrets` table using
`sober-crypto` envelope encryption (MEK/DEK), the same pattern as MCP
server credentials. Each platform's secrets are stored with
`secret_type = "gateway_platform"` and `metadata.platform_id = "<uuid>"`.

The `credentials` column on `gateway_platforms` is removed — credentials
are not stored inline. The gateway decrypts credentials at startup and
on `Reload` using the system MEK.

Per-platform credential shapes:

| Platform | Fields |
|----------|--------|
| Discord | `bot_token` |
| Telegram | `bot_token` |
| Matrix | `homeserver_url`, `access_token` |
| WhatsApp | `phone_number_id`, `access_token` |

## Scope

### This Plan

- Gateway binary: event loop, mapping lookup, gRPC client, response buffering
- Event types: `GatewayEvent`, `PlatformMessage`
- Discord implementation: channels, threads, message editing, @mention detection
- Bridge bot user: seeded in migrations
- Data model: 3 tables + migrations
- API endpoints in sober-api: platform/mapping/user CRUD (`GatewayAdminService`)
- Web UI: gateway settings pages
- Metrics, dashboard, service files, docs

### Future Platforms (separate plans)

- Telegram
- Matrix
- WhatsApp

Each is a self-contained `PlatformBridge` implementation + cargo feature.

### Future Enhancements (not in scope)

- Auto-create conversations on first platform interaction
- Rich content (embeds, buttons, reactions)
- File/image attachments via blob store
- Per-channel tool restrictions
- Platform-specific commands (Discord slash commands)
- Typing indicators
