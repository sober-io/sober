# #049: Service Layer Extraction for sober-api

## Problem

Route handlers in `sober-api` mix HTTP concerns with business logic. Handlers
like `conversations.rs` (566 lines), `collaborators.rs` (363 lines), and
`messages.rs` (294 lines) contain workspace provisioning, authorization matrices,
event message creation, WebSocket broadcasting, batch N+1 avoidance, and response
assembly — all inline with HTTP extraction and response formatting.

This makes handlers hard to read, hard to test, and inconsistent. Some handlers
(like `auth.rs`) already delegate to a service (`AuthService`), but most don't.
Multi-table operations that should be atomic (per plan #042's design) use separate
transactions or no transactions at all.

## Solution

Extract business logic from every route handler into a `services/` module within
`sober-api`. Each domain gets a service struct holding `PgPool` + any needed
clients. Handlers become thin: parse HTTP input, call service, format response.

Services use the `_tx` pattern (introduced in #047) for multi-table operations:
`self.db.begin()` + `Repo::method_tx(&mut *tx, ...)` + `tx.commit()`.

## Architecture

```
Handler (thin)                  Service (business logic)
─────────────────               ────────────────────────
Extract HTTP input              Validate / authorize
  ↓                               ↓
Call service method    ←───→    Compose repo _tx calls in transactions
  ↓                               ↓
Wrap in ApiResponse             Return typed DTOs
```

### Services

| Service | Dependencies | Domain |
|---------|-------------|--------|
| `TagService` | `PgPool` | Tag CRUD, conversation/message tagging |
| `UserService` | `PgPool` | User search |
| `ConversationService` | `PgPool`, `AppConfig` | Conversation CRUD, settings, inbox, jobs |
| `CollaboratorService` | `PgPool`, `UserConnectionRegistry` | Group membership, events, broadcasts |
| `MessageService` | `PgPool` | Message listing (batch fetch), deletion |
| `WsDispatchService` | `PgPool`, `AgentClient`, `ConnectionRegistry` | WS message dispatch to agent |
| `PluginService` | `PgPool`, `AgentClient` | Plugin gRPC proxying |
| `EvolutionService` | `PgPool`, `AgentClient`, `AppConfig` | Evolution lifecycle, state machine |
| `AttachmentService` | `PgPool`, `Arc<BlobStore>` | Upload processing, serving |
| `AuthService` | `PgPool` | Inbox creation, user profile with roles |

### Not extracted

- `health.rs` / `system.rs` — operational endpoints, no domain logic.
- `workspaces.rs` — empty (12 lines), functionality moved to conversation settings.

### WS types relocation

`ServerWsMessage` and `CollaboratorInfo` are defined in `routes/ws.rs` but
imported by `connections.rs`, `subscribe.rs`, and `collaborators.rs`. Move to
a dedicated `ws_types.rs` module to break this coupling.

## Transaction Strategy

Services manage cross-repo transactions using the `_tx` pattern from #047.
Repos provide `_tx` method variants accepting `&mut PgConnection`. Services
call `self.db.begin()`, compose multiple `_tx` calls, then `tx.commit()`.

Operations requiring service-level transactions:

| Service | Method | `_tx` methods composed |
|---------|--------|----------------------|
| `ConversationService` | `create` | `provision_tx` + `create_tx` |
| `ConversationService` | `update_settings` | `update_agent_mode_tx` + `upsert_tx` |
| `ConversationService` | `clear_messages` | `clear_conversation_tx` + `reset_all_unread_tx` |
| `CollaboratorService` | `add` | `create_tx` (cu) + `create_tx` (event msg) |
| `CollaboratorService` | `update_role` | `update_role_tx` + `create_tx` (event msg) |
| `CollaboratorService` | `remove` / `leave` | `remove_collaborator_tx` + `create_tx` (event msg) + `convert_to_direct_tx` |
| `MessageService` | `delete` | `delete_tx` + `find_unreferenced_by_message_tx` + `delete_tx` (attachments) |

New `_tx` methods needed on `PgConversationUserRepo`: `create_tx`,
`remove_collaborator_tx`, `update_role_tx`. On `PgConversationRepo`:
`convert_to_direct_tx`.

Tag operations are individually idempotent (upserts / INSERT ON CONFLICT) and
don't need transactions — `TagService` uses pool-based repo calls directly.

## Response DTOs

Manual `serde_json::json!()` calls in handlers are replaced with typed
`#[derive(Serialize)]` structs returned by services. This gives type safety,
prevents silent field drift, and makes responses testable without JSON parsing.

Key DTOs:
- `CreateConversationResponse`, `UpdateConversationResponse`, `InboxResponse`
- `ConvertToGroupResponse`, `SettingsResponse` (moved from routes)
- `UserSearchResult`, `UserProfile`
- `MessageWithDetails` (replaces manual JSON assembly in `list_messages`)
- `PluginInfo`, `SkillInfo`, `ToolInfo`, `AuditLogEntry`
- `AttachmentContent`

## AppState Changes

Services are constructed in `AppState::new()` and `AppState::from_parts()`
from existing components (pool, clients, registries). Each wrapped in `Arc`.

## Testing

Existing integration tests (`#[sqlx::test]`) exercise the full handler stack.
After extraction, they validate that the service layer works correctly without
modification. Pure logic in services (like evolution state machine validation)
gets `#[cfg(test)]` unit tests directly in the service file.

## Documentation Updates

- **`ARCHITECTURE.md`**: Add service layer description to the `sober-api` section.
  Document that handlers delegate to services, services compose `_tx` repo methods
  in transactions. Update the crate map table entry for `sober-api`.
- **`docs/rust-patterns.md`**: Add "Service Layer Pattern" section documenting:
  service struct shape, `_tx` transaction composition, when to use transactions
  vs direct repo calls, response DTO pattern. Add "Transaction Composition" section
  documenting the `_tx` method convention and how services compose them.

This ensures future agents and sessions discover these patterns from project docs
rather than needing to grep the codebase.

## What stays the same

- Route definitions (`fn routes() -> Router<Arc<AppState>>`) stay in route files.
- HTTP-specific concerns (extractors, cookie handling, multipart parsing, `RequireAdmin`) stay in handlers.
- `sober-auth::AuthService` is unchanged — the new `services::AuthService` handles API-level concerns only.
- No new crates. Services live in `sober-api/src/services/`.
