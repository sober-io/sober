# Design 025: Conversation Improvements — Core

> Sub-spec 1 of 3. Covers: dashboard, inbox, unread tracking, pagination, tags,
> archiving, hard delete, slash commands, message deletion.
>
> Sub-spec 2: Session settings, scheduler attachment, move messages, message tag UI.
> Sub-spec 3: Group conversations, invitations, shared context.

## Context

The current conversation system is a basic chat interface — flat list, no
organization tools, no unread tracking, all messages loaded at once. This
redesign adds the features needed for a session-based workflow without changing
the core "conversation" concept.

## Decisions

- **Keep "conversation" naming** — no rename. The concept is accurate.
- **Conversation kinds**: `direct` (user + agent), `group` (multi-user, future),
  `inbox` (permanent catch-all per user).
- **Inbox**: one per user, created on user registration, undeletable. Migration
  creates inbox for all existing users.
- **Unread tracking**: materialized `unread_count` per user per conversation,
  incremented on new message, reset when user views.
- **Tags**: first-class `tags` table, applied to conversations and messages via
  junction tables. Freeform with auto-assigned colors, autocomplete from existing.
- **Pagination**: cursor-based, 50 messages per page, infinite scroll up.
- **Slash commands**: client-side only (`/help`, `/info`, `/clear`).
- **Auto-archive**: deferred to a later sub-spec.
- **Tag management (rename/recolor)**: deferred to a later sub-spec.
- **Message tag UI**: schema and API in sub-spec 1, frontend UI deferred to sub-spec 2.

---

## 1. Database Schema

### 1.1 New enum types

```sql
CREATE TYPE conversation_kind AS ENUM ('direct', 'group', 'inbox');
CREATE TYPE user_role AS ENUM ('owner', 'member');
```

### 1.2 `conversations` — new columns

| Column | Type | Default | Notes |
|--------|------|---------|-------|
| `kind` | `conversation_kind` | `'direct'` | |
| `is_archived` | `BOOLEAN` | `false` | |

```sql
CREATE UNIQUE INDEX idx_conversations_inbox
  ON conversations (user_id) WHERE kind = 'inbox';

CREATE INDEX idx_conversations_archived
  ON conversations (user_id, is_archived);
```

**Note on `conversations.user_id`:** The existing `user_id` column remains as a
denormalized shortcut for fast owner queries on direct conversations. The
canonical ownership source is `conversation_users.role = 'owner'`. Sub-spec 3
(group conversations) will evaluate whether to keep or drop this column.

### 1.3 `messages` — new column

| Column | Type | Notes |
|--------|------|-------|
| `user_id` | `UUID` | FK → users, nullable. NULL for assistant/system/tool messages. |

### 1.4 New table: `conversation_users`

| Column | Type | Notes |
|--------|------|-------|
| `conversation_id` | `UUID` | FK → conversations, CASCADE |
| `user_id` | `UUID` | FK → users, CASCADE |
| `unread_count` | `INTEGER` | Default 0 |
| `last_read_at` | `TIMESTAMPTZ` | NULL = never read |
| `role` | `user_role` | `owner` or `member` |
| `joined_at` | `TIMESTAMPTZ` | |
| PK | `(conversation_id, user_id)` | |

### 1.5 New table: `tags`

| Column | Type | Notes |
|--------|------|-------|
| `id` | `UUID` | PK |
| `user_id` | `UUID` | FK → users, CASCADE |
| `name` | `TEXT` | |
| `color` | `TEXT` | Hex, auto-assigned from palette |
| `created_at` | `TIMESTAMPTZ` | |
| Unique | `(user_id, name)` | |

### 1.6 New table: `conversation_tags`

| Column | Type | Notes |
|--------|------|-------|
| `conversation_id` | `UUID` | FK → conversations, CASCADE |
| `tag_id` | `UUID` | FK → tags, CASCADE |
| PK | `(conversation_id, tag_id)` | |

### 1.7 New table: `message_tags`

| Column | Type | Notes |
|--------|------|-------|
| `message_id` | `UUID` | FK → messages, CASCADE |
| `tag_id` | `UUID` | FK → tags, CASCADE |
| PK | `(message_id, tag_id)` | |

### 1.8 Indexes

```sql
-- Efficient cursor-based pagination (replaces existing conversation_id-only index)
CREATE INDEX idx_messages_conversation_id_desc
  ON messages (conversation_id, id DESC);
```

### 1.9 Key behaviors

- Every new conversation → insert `conversation_users` row with role `owner`.
- Inbox created during user registration. `GET /api/v1/conversations/inbox`
  returns it (no lazy creation needed).
- New message → `UPDATE conversation_users SET unread_count = unread_count + 1
  WHERE conversation_id = $1 AND user_id != $2` (exclude sender).
- User views conversation → `SET unread_count = 0, last_read_at = now()`.
- Tag creation is idempotent: `INSERT INTO tags ... ON CONFLICT (user_id, name)
  DO NOTHING`, then select the id, then insert junction row.
- Tag autocomplete: `SELECT id, name, color FROM tags WHERE user_id = $1`.
- Clearing messages (`/clear`) resets `unread_count = 0` for all users in
  that conversation. The agent will start fresh with no history context.
- Archiving/unarchiving and tagging do NOT touch `conversations.updated_at` —
  only new messages update it. This keeps sidebar ordering based on activity.

### 1.10 Data migrations

The migration must backfill existing data:

1. **`conversation_users` for existing conversations** — insert a row for each
   existing conversation with `user_id` from `conversations.user_id`,
   `role = 'owner'`, `unread_count = 0`, `last_read_at = now()`,
   `joined_at = conversations.created_at`.

2. **`messages.user_id` for existing messages** — set `user_id` to the
   conversation owner (`conversations.user_id`) for all messages where
   `role = 'user'`. Leave NULL for `assistant`, `system`, and `tool` messages.

3. **Inbox for existing users** — create an inbox conversation (`kind = 'inbox'`)
   for every existing user, plus a corresponding `conversation_users` row.

### 1.11 New ID newtypes

Add `TagId` via `define_id!` macro in `sober-core/src/types/ids.rs`.

---

## 2. Backend API

### 2.1 Modified endpoints

**`GET /api/v1/conversations`** — new query params:
- `?archived=true|false` (default: false)
- `?kind=direct|inbox|group`
- `?tag=<name>`
- `?search=<query>` (title search, `ILIKE '%query%'`; add `pg_trgm` GIN index
  on `conversations.title` if performance degrades at scale)
- Response includes: `unread_count`, `kind`, `is_archived`, `tags[]`.

**`GET /api/v1/conversations/{id}`** — response includes `unread_count`, `kind`,
`is_archived`, `tags[]`, `users[]`. No longer returns messages inline — use the
paginated messages endpoint. The `ConversationWithMessages` frontend type is
removed; the page loader fetches conversation and first message page separately.

**`POST /api/v1/conversations`** — now sets `kind = 'direct'` (default) and
creates a `conversation_users` row with role `owner` for the authenticated user.

**`DELETE /api/v1/conversations/{id}`** — returns 403 if `kind = 'inbox'`.

### 2.2 New endpoints

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/v1/conversations/inbox` | Get or create user's inbox |
| `POST` | `/api/v1/conversations/{id}/read` | Mark as read (reset unread) |
| `PATCH` | `/api/v1/conversations/{id}` | Extended: accepts `archived` field alongside existing `title` and `permission_mode` |
| `DELETE` | `/api/v1/conversations/{id}/messages` | Clear all messages (`/clear`) |
| `GET` | `/api/v1/conversations/{id}/messages?before=<cursor>&limit=50` | Paginated messages |
| `GET` | `/api/v1/tags` | List user's tags |
| `POST` | `/api/v1/conversations/{id}/tags` | Add tag to conversation |
| `DELETE` | `/api/v1/conversations/{id}/tags/{tag_id}` | Remove tag from conversation |
| `POST` | `/api/v1/messages/{id}/tags` | Add tag to message |
| `DELETE` | `/api/v1/messages/{id}/tags/{tag_id}` | Remove tag from message |
| `DELETE` | `/api/v1/messages/{id}` | Delete single message |

### 2.3 Pagination

Cursor-based using message `id` (UUIDv7 = naturally time-ordered). `?before=<uuid>&limit=50`
returns 50 messages before cursor. First load omits `before` for latest 50.

### 2.4 Message deletion authorization

A message can be deleted by:
- The conversation owner (any message).
- The user who sent the message (`messages.user_id`).

### 2.5 Inbox lifecycle

`GET /api/v1/conversations/inbox` returns the user's inbox conversation.
The inbox is created during user registration, so this endpoint is a simple
lookup (not a get-or-create).

---

## 3. WebSocket Changes

### 3.1 User-level connection tracking

The existing `ConnectionRegistry` is keyed by `conversation_id` and cannot route
messages to a user across conversations. Add a `UserConnectionRegistry` that
maps `user_id → Vec<mpsc::Sender<ServerWsMessage>>`.

- On WebSocket open (after auth) → register sender in `UserConnectionRegistry`.
- On WebSocket close → unregister.
- `chat.unread` events are sent via the user registry, not the conversation registry.

This is a lightweight addition — the existing per-conversation registry remains
for streaming events (`chat.delta`, `chat.done`, etc.).

### 3.2 New server → client message

```
{ "type": "chat.unread", "conversation_id": "<uuid>", "unread_count": <int> }
```

Sent via `UserConnectionRegistry` when a user's unread count changes on a
conversation they don't have an active subscription to.

### 3.3 Unread integration

- Agent/scheduler produces a message → API increments `unread_count` for users
  without an active subscription to that conversation → sends `chat.unread` via
  user registry.
- Client sends `chat.subscribe` → WebSocket handler calls the read repo method
  directly (idempotent: `SET unread_count = 0, last_read_at = now()`).

---

## 4. Frontend — Dashboard

Replaces the placeholder at `/` (root route). First screen after login.

### 4.1 Components

- **Search bar** — searches conversation titles and tags, real-time filtering.
- **Unread section** — conversations with `unread_count > 0`, sorted by most
  recent, shows badge count. Collapses when empty ("All caught up").
- **Recent conversations** — last 10-20 active (non-archived), sorted by
  `updated_at`.
- **Quick actions** — "New conversation" button, inbox link.

### 4.2 Behavior

- Unread section updates in real-time via `chat.unread` WebSocket events.
- Click navigates to `/chat/[id]` and marks as read.
- Search results include archived conversations (visually distinguished).

---

## 5. Frontend — Sidebar Enhancements

### 5.1 Additions

- **Unread badges** — pill/count next to conversations with unread > 0.
- **Inbox pinned at top** — always visible, distinct icon, never scrolls.
- **Archive toggle** — button at bottom to show/hide archived conversations.
- **Tag pills** — small colored dots next to conversation titles.

### 5.2 Ordering

Unread conversations float to top, then sorted by `updated_at` descending.
Archived hidden by default.

No search in sidebar — search lives on the dashboard.

---

## 6. Frontend — Chat Page Changes

### 6.1 Pagination

- Initial load: latest 50 messages.
- Scroll to top: load 50 more via `?before=<cursor>&limit=50`.
- Loading spinner at top while fetching.
- Scroll position preserved after load (no jump).
- Stop when API returns < 50 (all history loaded).

### 6.2 Conversation tags

- Tag area below title or in dropdown menu.
- Click to add — input with autocomplete from existing tags.
- New name auto-creates tag with assigned color.
- Click tag pill to remove.

### 6.3 Message tags

Deferred to sub-spec 2. The `message_tags` schema (section 1.7) and API
endpoints (`POST /messages/{id}/tags`, `DELETE /messages/{id}/tags/{tag_id}`)
are implemented in sub-spec 1, but the frontend UI is not.

### 6.4 Message deletion

- Hover/right-click message reveals delete action.
- Confirmation prompt before deleting.
- Authorized for conversation owner or message sender.

### 6.5 Slash commands

- Input starting with `/` triggers command palette overlay.
- `/help` — shows available commands (ephemeral, not sent to agent).
- `/info` — shows conversation metadata (kind, created, message count, tags, users).
- `/clear` — confirmation dialog, then `DELETE /conversations/{id}/messages`.
  Agent starts fresh with no conversation history after clear.
- All client-side — never sent to the agent.

### 6.6 Mark as read

- Automatic when chat page loads (via `chat.subscribe` → read).
- Sidebar unread badge clears immediately.

---

## 7. Frontend — Archive & Delete

### 7.1 Archiving

- Available from conversation context menu ("..." in sidebar) and chat page header.
- Toggle via existing PATCH endpoint (`{ "archived": true|false }`).
- Archived conversations hidden from sidebar, visible via archive toggle.
- Archived conversations appear in dashboard search (visually marked).

### 7.2 Hard delete

- Available from context menu and chat page header.
- Confirmation dialog: "This will permanently delete this conversation and all
  messages. This cannot be undone."
- Blocked for inbox conversations (hidden or disabled).
- Navigates to dashboard after deletion.
