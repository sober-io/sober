# Design 027: Group Conversations

> Sub-spec 3 of 3. Covers: group conversations, member management, agent mode,
> conversation timeline events.
>
> Depends on: sub-spec 1 (#025) and sub-spec 2 (#026).

## Context

Sub-specs 1 and 2 laid the foundation: `conversation_users` with roles,
`conversation_kind` with `group` variant, `user_role` enum, and the settings
panel. This sub-spec activates group conversations — multiple users in one
conversation with configurable agent behavior.

## Decisions

- **Direct add** — owner/admin adds members by username. No invite/accept flow.
- **Three roles**: `owner`, `admin`, `member`. Owner has full control, admin can
  manage members and settings, member can chat and tag.
- **Agent mode per conversation**: `always`, `mention` (`@sober`), `silent`.
  Direct/inbox conversations always behave as `always`.
- **Full history** visible to new members. No per-user visibility boundary.
- **Self-leave** for members and admins. Owner cannot leave (must delete).
- **No member limit.**
- **Owner transfer** — deferred.
- **Timeline events** — member changes and other significant actions stored as
  messages with `role = 'event'` and a general `metadata` JSONB field.

---

## 1. Database Schema

### 1.1 Alter existing enums

```sql
ALTER TYPE user_role ADD VALUE 'admin';
```

### 1.2 New enum

```sql
CREATE TYPE agent_mode AS ENUM ('always', 'mention', 'silent');
```

### 1.3 `conversations` — new column

| Column | Type | Default | Notes |
|--------|------|---------|-------|
| `agent_mode` | `agent_mode` | `'always'` | Only meaningful for group conversations |

### 1.4 `messages` — new columns

| Column | Type | Notes |
|--------|------|-------|
| `metadata` | `JSONB` | Nullable. General-purpose metadata for any message role |

### 1.5 `message_role` enum — new value

```sql
ALTER TYPE message_role ADD VALUE 'event';
```

### 1.6 `conversations.user_id` column

The existing `user_id` column on `conversations` remains as a denormalized
shortcut for direct/inbox conversations. For group conversations, it is set to
the creator's user_id at creation time and never updated (ownership is managed
via `conversation_users`). No schema change — just documented behavior.

### 1.7 No other schema changes

`conversation_users` already supports multiple users with roles.
`conversation_kind` already has `group`. `messages.user_id` already tracks
senders.

---

## 2. Backend API — Member Management

### 2.1 New endpoints

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/v1/conversations/{id}/members` | List members with roles |
| `POST` | `/api/v1/conversations/{id}/members` | Add member(s) by username |
| `PATCH` | `/api/v1/conversations/{id}/members/{user_id}` | Change member role |
| `DELETE` | `/api/v1/conversations/{id}/members/{user_id}` | Remove member (kick) |
| `POST` | `/api/v1/conversations/{id}/leave` | Self-leave |

### 2.2 Authorization rules

| Action | Who |
|--------|-----|
| Send messages | Any member |
| List members | Any member |
| Add member | Owner, admin |
| Change role to admin | Owner only |
| Demote admin to member | Owner only |
| Remove member | Owner, admin (admin can't remove owner or other admins) |
| Leave | Any member except owner |

**Message-sending authorization:** All conversation-scoped endpoints (sending
messages, subscribing via WebSocket, etc.) must check `conversation_users`
membership instead of `conversations.user_id`. This is a cross-cutting change
that affects the WebSocket handler (`chat.subscribe`, `chat.message`) and the
API handlers. Non-members are rejected with 404.

**Adding existing members:** Idempotent — returns the existing membership
without error. No duplicate event message inserted.

### 2.3 Timeline events

Every member change inserts a message with `role = 'event'`:

- **Content** (human-readable): `"Alice added Bob"`, `"Carol left"`,
  `"Alice changed Bob's role to admin"`, `"Alice removed Dave"`
- **Metadata** (machine-readable):
  ```json
  {
    "type": "member_added|member_removed|role_changed",
    "actor_id": "uuid",
    "target_id": "uuid",
    "target_username": "...",
    "role": "member|admin"
  }
  ```
  For `member_left`, metadata omits `target_id`/`target_username` (actor is
  the one leaving):
  ```json
  { "type": "member_left", "actor_id": "uuid" }
  ```
- `user_id` on the message is set to the actor (person who performed the action)
- These messages appear in the timeline alongside regular messages, paginated
  normally

### 2.4 Create group conversation

Extend `POST /api/v1/conversations` to accept:
- `kind: 'group'` (optional, defaults to `'direct'`)
- `title: "..."` (required for group conversations)
- `members: [{ username: "..." }]` (optional, only for group)

Creates conversation with `kind = 'group'`, adds creator as `owner`, adds
listed users as `member`. Inserts event messages for each added member.
Title is required for groups — direct conversations can be untitled but groups
need a name for identification.

### 2.5 Agent mode

Add `agent_mode` to `PATCH /api/v1/conversations/{id}` request body.
Owner/admin can change. Ignored for direct/inbox conversations (always
`always`).

---

## 3. WebSocket Changes

### 3.1 New server → client messages

```json
{ "type": "chat.member_added", "conversation_id": "...", "user": { "id": "...", "username": "..." }, "role": "member" }
{ "type": "chat.member_removed", "conversation_id": "...", "user_id": "..." }
{ "type": "chat.role_changed", "conversation_id": "...", "user_id": "...", "role": "admin" }
```

**Routing:** The API handler iterates `conversation_users` for the conversation
and sends the event via `UserConnectionRegistry` to each member. This ensures
members who aren't actively viewing the conversation still receive the event
(for sidebar/dashboard updates).

**Kicked user handling:** When a member is removed, `chat.member_removed` is
sent to the kicked user (with their own `user_id`). The frontend detects this,
unsubscribes from the conversation, and navigates to the dashboard/inbox with
a notice.

### 3.2 Existing events work as-is

`chat.delta`, `chat.new_message`, etc. already carry `conversation_id` and
route through `ConnectionRegistry`. Multiple users subscribing to the same
conversation all receive events.

Unread tracking from sub-spec 1 already works for groups —
`increment_unread` excludes the sender.

---

## 4. Frontend — Group Conversation UI

### 4.1 Creating a group conversation

The "New conversation" button becomes a dropdown:
- "New direct" — current behavior
- "New group" — dialog with title input and member autocomplete (add by
  username). Creates with `kind = 'group'`.

### 4.2 Member management in settings panel

New section in `ConversationSettings` (between tags and scheduled jobs):
- Shows all members: username, role badge (owner/admin/member)
- **Owner/admin view**: "Add member" input (username autocomplete), role
  change dropdown per member, remove button
- **Member view**: read-only list, "Leave" button at bottom
- **Owner**: no leave button

### 4.3 Agent mode in settings panel

New section (between permission mode and workspace):
- Three-option selector: Always / Mention / Silent
- Only visible for group conversations
- Owner/admin can change, members see read-only

### 4.4 Message display in group conversations

- User messages show sender's username above the message bubble
- Event messages (`role = 'event'`) render as centered, muted text with no
  bubble — e.g., "Alice added Bob"

### 4.5 Components

- **New:** `MemberList.svelte` — member list with role badges, add/remove/role
  change controls
- **New:** `AddMemberInput.svelte` — username autocomplete for adding members
- **New:** `CreateGroupDialog.svelte` — dialog for new group creation
- **Modify:** `ConversationSettings.svelte` — add members and agent mode sections
- **Modify:** `ChatMessage.svelte` — render event messages, show sender username
  in groups
- **Modify:** `ConversationList.svelte` — visual distinction for group
  conversations (e.g., group icon)

---

## 5. Agent — Mention Detection and Mode Handling

### 5.1 Mode check

When `sober-agent` processes a message from a group conversation, it checks
`agent_mode` after storing the user message but before running the LLM
pipeline:
- `always` — process every message (same as direct)
- `mention` — only process if content contains `@sober` (case-insensitive
  substring match, literal typing — no autocomplete or special formatting)
- `silent` — never respond. Messages stored only. Scheduler jobs can still
  trigger agent via `WakeAgent`

### 5.2 Implementation location

In the agent's `handle_message` flow, after message storage and injection
checking. The agent looks up the conversation (new dependency: conversation
repo access) to get `agent_mode`. If the mode says don't respond, the agent
returns the stored `message_id` in `HandleMessageResponse` as normal but skips
the LLM pipeline. The caller (API) receives the same response shape regardless
— the difference is that no `ConversationUpdate` events are published.

### 5.3 Prompt assembly

`sober-mind` must:
- **Exclude `event` messages** from the LLM prompt. Event messages have
  `role = 'event'` which is not a valid LLM role. Filter them out during
  prompt assembly.
- **Prefix user messages with usernames** in group conversations so the LLM
  can distinguish between different users: e.g., `"[Alice]: How do I..."`.

### 5.4 No gRPC protocol changes

The agent already receives all messages. Filtering is internal.

### 5.5 Empty groups

When all non-owner members leave a group conversation, it remains
`kind = 'group'` with the current `agent_mode`. No auto-conversion to
`direct`. The owner can add new members or delete the conversation.

---

## 6. Repository Changes

### 6.1 New traits / methods

**`ConversationUserRepo`** (extend from sub-spec 1):
- `list_members(conversation_id) -> Result<Vec<ConversationUserWithUsername>, AppError>`
  (joins with users table for username)
- `update_role(conversation_id, user_id, role) -> Result<(), AppError>`
- `delete(conversation_id, user_id) -> Result<(), AppError>` (remove member)

**`ConversationRepo`** (extend):
- `update_agent_mode(id, agent_mode) -> Result<(), AppError>`

**`UserRepo`** (extend):
- `find_by_username(username) -> Result<User, AppError>` (for member lookup)

### 6.2 New domain types

- `ConversationUserWithUsername` — lightweight join type (row-level, not a full
  domain struct): `ConversationUser` fields + `username: String`
- `AgentMode` enum — `Always`, `Mention`, `Silent` (with sqlx type mapping)
- Add `agent_mode: AgentMode` to `Conversation` domain struct
- Add `metadata: Option<serde_json::Value>` to `Message` domain struct
