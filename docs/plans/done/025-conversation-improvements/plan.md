# 025: Conversation Improvements — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add conversation organization features: inbox, unread tracking, tags, pagination, archiving, slash commands, and message deletion.

**Architecture:** Extend the existing conversation system with new DB tables (conversation_users, tags, junction tables), new enums (conversation_kind, user_role), cursor-based pagination, a UserConnectionRegistry for cross-conversation unread notifications, and frontend dashboard/sidebar/chat improvements.

**Tech Stack:** Rust (sqlx, axum, tonic), Svelte 5, TypeScript, PostgreSQL, Tailwind v4

**Design doc:** `docs/plans/pending/025-conversation-improvements/design.md`

---

## File Structure

### Backend — sober-core (types)

| File | Action | Responsibility |
|------|--------|---------------|
| `backend/crates/sober-core/src/types/ids.rs` | Modify | Add `TagId` |
| `backend/crates/sober-core/src/types/enums.rs` | Modify | Add `ConversationKind`, `ConversationUserRole` |
| `backend/crates/sober-core/src/types/domain.rs` | Modify | Add `kind`, `is_archived` to `Conversation`; add `user_id` to `Message`; new structs: `ConversationUser`, `Tag`, `ConversationWithDetails` |
| `backend/crates/sober-core/src/types/input.rs` | Modify | Add `CreateTag`, `ListConversationsFilter` |
| `backend/crates/sober-core/src/types/repo.rs` | Modify | Extend `ConversationRepo` and `MessageRepo` traits; add `TagRepo`, `ConversationUserRepo` |
| `backend/crates/sober-core/src/types/mod.rs` | Modify | Re-export new types if needed |

### Backend — sober-db (repos)

| File | Action | Responsibility |
|------|--------|---------------|
| `backend/crates/sober-db/src/repos/conversations.rs` | Modify | Extend `PgConversationRepo` with new methods |
| `backend/crates/sober-db/src/repos/messages.rs` | Modify | Add cursor pagination, delete, clear, user_id handling |
| `backend/crates/sober-db/src/repos/tags.rs` | Create | `PgTagRepo` implementation |
| `backend/crates/sober-db/src/repos/conversation_users.rs` | Create | `PgConversationUserRepo` implementation |
| `backend/crates/sober-db/src/rows.rs` | Modify | Add row types for new tables; update ConversationRow, MessageRow |
| `backend/crates/sober-db/src/repos/mod.rs` | Modify | Re-export new repos |

### Backend — sober-api

| File | Action | Responsibility |
|------|--------|---------------|
| `backend/crates/sober-api/src/routes/conversations.rs` | Modify | Update existing handlers; add inbox, mark-read, archive, clear-messages |
| `backend/crates/sober-api/src/routes/messages.rs` | Create | Paginated messages, message deletion, message tags |
| `backend/crates/sober-api/src/routes/tags.rs` | Create | Tag CRUD + conversation/message tag endpoints |
| `backend/crates/sober-api/src/routes/auth.rs` | Modify | Create inbox on user registration |
| `backend/crates/sober-api/src/routes/mod.rs` | Modify | Register new route modules |
| `backend/crates/sober-api/src/routes/ws.rs` | Modify | Add `chat.unread` server message; mark-read on subscribe |
| `backend/crates/sober-api/src/connections.rs` | Modify | Add `UserConnectionRegistry` |
| `backend/crates/sober-api/src/state.rs` | Modify | Add `user_connections` and new repos to `AppState` |
| `backend/crates/sober-api/src/subscribe.rs` | Modify | Wire unread notifications via `UserConnectionRegistry` |
| `backend/crates/sober-api/src/lib.rs` | Modify | Register new routes |

### Database

| File | Action | Responsibility |
|------|--------|---------------|
| `backend/migrations/YYYYMMDD000001_conversation_improvements.sql` | Create | All schema changes + data backfill in one migration |

### Frontend

| File | Action | Responsibility |
|------|--------|---------------|
| `frontend/src/lib/types/index.ts` | Modify | Add new TS types, update Conversation/Message |
| `frontend/src/lib/services/conversations.ts` | Modify | Add inbox, mark-read, archive, clear, paginated messages |
| `frontend/src/lib/services/tags.ts` | Create | Tag API service |
| `frontend/src/lib/stores/conversations.svelte.ts` | Modify | Add unread tracking, archive filter, inbox |
| `frontend/src/lib/stores/websocket.svelte.ts` | Modify | Handle `chat.unread` messages |
| `frontend/src/routes/(app)/+page.svelte` | Modify | Dashboard page (replace placeholder) |
| `frontend/src/routes/(app)/+page.ts` | Create | Dashboard loader |
| `frontend/src/routes/(app)/+layout.svelte` | Modify | Sidebar enhancements (unread badges, inbox, archive toggle, tags) |
| `frontend/src/routes/(app)/chat/[id]/+page.svelte` | Modify | Pagination, tags, message deletion, slash commands |
| `frontend/src/routes/(app)/chat/[id]/+page.ts` | Modify | Separate conversation + messages loading |
| `frontend/src/lib/components/SlashCommandPalette.svelte` | Create | Slash command overlay |
| `frontend/src/lib/components/TagInput.svelte` | Create | Tag autocomplete input |
| `frontend/src/lib/components/ConfirmDialog.svelte` | Create | Reusable confirmation dialog |

---

## Chunk 1: Database Migration & Core Types

### Task 1: SQL Migration

**Files:**
- Create: `backend/migrations/YYYYMMDD000001_conversation_improvements.sql`

- [ ] **Step 1: Move plan to active/**

Per CLAUDE.md plan lifecycle rules, move the plan folder from pending/ to active/ in the first commit:

```bash
git mv docs/plans/pending/025-conversation-improvements docs/plans/active/025-conversation-improvements
```

- [ ] **Step 2: Write the migration SQL**

Use `cd backend && sqlx migrate add conversation_improvements` to create the file, then populate it with:

```sql
-- New enum types
CREATE TYPE conversation_kind AS ENUM ('direct', 'group', 'inbox');
CREATE TYPE user_role AS ENUM ('owner', 'member');

-- conversations: add kind and is_archived
ALTER TABLE conversations
  ADD COLUMN kind conversation_kind NOT NULL DEFAULT 'direct',
  ADD COLUMN is_archived BOOLEAN NOT NULL DEFAULT false;

CREATE UNIQUE INDEX idx_conversations_inbox
  ON conversations (user_id) WHERE kind = 'inbox';

CREATE INDEX idx_conversations_archived
  ON conversations (user_id, is_archived);

-- messages: add user_id
ALTER TABLE messages
  ADD COLUMN user_id UUID REFERENCES users(id) ON DELETE SET NULL;

-- conversation_users
CREATE TABLE conversation_users (
  conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  unread_count    INTEGER NOT NULL DEFAULT 0,
  last_read_at    TIMESTAMPTZ,
  role            user_role NOT NULL DEFAULT 'member',
  joined_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (conversation_id, user_id)
);

-- tags
CREATE TABLE tags (
  id         UUID PRIMARY KEY,
  user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  color      TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (user_id, name)
);

-- conversation_tags
CREATE TABLE conversation_tags (
  conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  tag_id          UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (conversation_id, tag_id)
);

-- message_tags
CREATE TABLE message_tags (
  message_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  tag_id     UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (message_id, tag_id)
);

-- Efficient cursor-based pagination
CREATE INDEX idx_messages_conversation_id_desc
  ON messages (conversation_id, id DESC);

-- Backfill: conversation_users for existing conversations
INSERT INTO conversation_users (conversation_id, user_id, role, unread_count, last_read_at, joined_at)
SELECT id, user_id, 'owner', 0, now(), created_at
FROM conversations;

-- Backfill: messages.user_id for existing user messages
UPDATE messages m
SET user_id = c.user_id
FROM conversations c
WHERE m.conversation_id = c.id
  AND m.role = 'user';

-- Backfill: create inbox for every existing user
WITH new_inboxes AS (
  INSERT INTO conversations (id, user_id, title, kind, created_at, updated_at)
  SELECT gen_random_uuid(), u.id, NULL, 'inbox', now(), now()
  FROM users u
  WHERE NOT EXISTS (
    SELECT 1 FROM conversations c WHERE c.user_id = u.id AND c.kind = 'inbox'
  )
  RETURNING id, user_id
)
INSERT INTO conversation_users (conversation_id, user_id, role, unread_count, last_read_at, joined_at)
SELECT id, user_id, 'owner', 0, now(), now()
FROM new_inboxes;
```

- [ ] **Step 3: Verify migration syntax**

Run: `cd backend && cargo sqlx migrate run` (requires Docker + running DB)

- [ ] **Step 4: Commit**

```
feat(db): add conversation improvements migration

Moves plan to active/. New tables: conversation_users, tags, conversation_tags,
message_tags. New columns: conversations.kind, conversations.is_archived,
messages.user_id. Backfills existing data and creates inbox for all users.
```

---

### Task 2: Core Types — IDs and Enums

**Files:**
- Modify: `backend/crates/sober-core/src/types/ids.rs:147` (after `EncryptionKeyId`)
- Modify: `backend/crates/sober-core/src/types/enums.rs`

- [ ] **Step 1: Add `TagId` to ids.rs**

After `EncryptionKeyId` (line 151), add:

```rust
define_id!(
    /// Unique identifier for a user-created tag.
    TagId
);
```

- [ ] **Step 2: Add `ConversationKind` and `ConversationUserRole` enums to enums.rs**

After the `MessageRole` enum (after line 80), add:

```rust
/// The kind/type of a conversation.
///
/// Maps to the `conversation_kind` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "conversation_kind", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum ConversationKind {
    /// A one-on-one conversation between user and agent.
    Direct,
    /// A multi-user conversation (future).
    Group,
    /// The user's permanent catch-all inbox.
    Inbox,
}

/// The role a user holds in a conversation.
///
/// Maps to the `user_role` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "user_role", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum ConversationUserRole {
    /// Conversation creator/owner.
    Owner,
    /// Regular participant.
    Member,
}
```

- [ ] **Step 3: Add serde roundtrip tests for new enums**

```rust
#[test]
fn conversation_kind_serde_roundtrip() {
    for variant in [
        ConversationKind::Direct,
        ConversationKind::Group,
        ConversationKind::Inbox,
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        let deserialized: ConversationKind = serde_json::from_str(&json).unwrap();
        assert_eq!(variant, deserialized);
    }
}

#[test]
fn conversation_user_role_serde_roundtrip() {
    for variant in [ConversationUserRole::Owner, ConversationUserRole::Member] {
        let json = serde_json::to_string(&variant).unwrap();
        let deserialized: ConversationUserRole = serde_json::from_str(&json).unwrap();
        assert_eq!(variant, deserialized);
    }
}
```

- [ ] **Step 4: Update imports in enums.rs re-exports and types/mod.rs**

Ensure `ConversationKind` and `ConversationUserRole` are re-exported from the types module.

- [ ] **Step 5: Run tests**

Run: `cd backend && cargo test -p sober-core -q`

- [ ] **Step 6: Commit**

```
feat(core): add TagId, ConversationKind, ConversationUserRole types
```

---

### Task 3: Core Types — Domain Structs

**Files:**
- Modify: `backend/crates/sober-core/src/types/domain.rs`

- [ ] **Step 1: Update `Conversation` struct**

Add `kind` and `is_archived` fields:

```rust
pub struct Conversation {
    pub id: ConversationId,
    pub user_id: UserId,
    pub title: Option<String>,
    pub workspace_id: Option<WorkspaceId>,
    pub kind: ConversationKind,
    pub is_archived: bool,
    pub permission_mode: crate::PermissionMode,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

- [ ] **Step 2: Update `Message` struct**

Add `user_id` field:

```rust
pub struct Message {
    pub id: MessageId,
    pub conversation_id: ConversationId,
    pub role: MessageRole,
    pub content: String,
    pub tool_calls: Option<serde_json::Value>,
    pub tool_result: Option<serde_json::Value>,
    pub token_count: Option<i32>,
    pub user_id: Option<UserId>,
    pub created_at: DateTime<Utc>,
}
```

- [ ] **Step 3: Add new domain structs**

After `Message`, add:

```rust
/// A user's membership in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationUser {
    /// The conversation.
    pub conversation_id: ConversationId,
    /// The user.
    pub user_id: UserId,
    /// Number of unread messages.
    pub unread_count: i32,
    /// When the user last read this conversation.
    pub last_read_at: Option<DateTime<Utc>>,
    /// The user's role in this conversation.
    pub role: ConversationUserRole,
    /// When the user joined.
    pub joined_at: DateTime<Utc>,
}

/// A user-created tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    /// Unique identifier.
    pub id: TagId,
    /// The user who owns this tag.
    pub user_id: UserId,
    /// Tag display name.
    pub name: String,
    /// Hex color code.
    pub color: String,
    /// When the tag was created.
    pub created_at: DateTime<Utc>,
}

/// A conversation with additional details for list/detail views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationWithDetails {
    /// The base conversation.
    #[serde(flatten)]
    pub conversation: Conversation,
    /// Number of unread messages for the requesting user.
    pub unread_count: i32,
    /// Tags applied to this conversation.
    pub tags: Vec<Tag>,
    /// Users in this conversation (populated for detail view, empty for list view).
    pub users: Vec<ConversationUser>,
}
```

- [ ] **Step 4: Update existing tests that construct Conversation/Message**

Add the new fields with defaults (`kind: ConversationKind::Direct`, `is_archived: false`, `user_id: None`).

- [ ] **Step 5: Run tests**

Run: `cd backend && cargo test -p sober-core -q`

- [ ] **Step 6: Run clippy**

Run: `cd backend && cargo clippy -p sober-core -q -- -D warnings`

- [ ] **Step 7: Commit**

```
feat(core): add conversation kind, archive, unread, and tag domain types
```

---

### Task 4: Core Types — Input Types and Repo Traits

**Files:**
- Modify: `backend/crates/sober-core/src/types/input.rs`
- Modify: `backend/crates/sober-core/src/types/repo.rs`

- [ ] **Step 1: Add input types to input.rs**

```rust
/// Filter parameters for listing conversations.
#[derive(Debug, Clone, Default)]
pub struct ListConversationsFilter {
    /// Filter by archived status.
    pub archived: Option<bool>,
    /// Filter by conversation kind.
    pub kind: Option<super::enums::ConversationKind>,
    /// Filter by tag name.
    pub tag: Option<String>,
    /// Search by title (ILIKE).
    pub search: Option<String>,
}

/// Input for creating a new tag (idempotent).
#[derive(Debug, Clone)]
pub struct CreateTag {
    /// The user who owns the tag.
    pub user_id: UserId,
    /// Tag name.
    pub name: String,
    /// Hex color code.
    pub color: String,
}
```

- [ ] **Step 2: Extend `ConversationRepo` trait**

Add these methods after existing ones:

```rust
/// Lists conversations for a user with filters, including unread counts and tags.
fn list_with_details(
    &self,
    user_id: UserId,
    filter: ListConversationsFilter,
) -> impl Future<Output = Result<Vec<ConversationWithDetails>, AppError>> + Send;

/// Gets the user's inbox conversation.
fn get_inbox(
    &self,
    user_id: UserId,
) -> impl Future<Output = Result<Conversation, AppError>> + Send;

/// Updates the archived status of a conversation.
fn update_archived(
    &self,
    id: ConversationId,
    archived: bool,
) -> impl Future<Output = Result<(), AppError>> + Send;
```

- [ ] **Step 3: Extend `MessageRepo` trait**

Add these methods:

```rust
/// Lists messages with cursor-based pagination (newest first before cursor).
fn list_paginated(
    &self,
    conversation_id: ConversationId,
    before: Option<MessageId>,
    limit: i64,
) -> impl Future<Output = Result<Vec<Message>, AppError>> + Send;

/// Deletes a single message by ID.
fn delete(
    &self,
    id: MessageId,
) -> impl Future<Output = Result<(), AppError>> + Send;

/// Deletes all messages in a conversation.
fn clear_conversation(
    &self,
    conversation_id: ConversationId,
) -> impl Future<Output = Result<(), AppError>> + Send;

/// Gets a single message by ID.
fn get_by_id(
    &self,
    id: MessageId,
) -> impl Future<Output = Result<Message, AppError>> + Send;
```

- [ ] **Step 4: Add `ConversationUserRepo` trait**

```rust
/// Conversation membership and unread tracking operations.
pub trait ConversationUserRepo: Send + Sync {
    /// Creates a membership row (e.g., when a conversation is created).
    fn create(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        role: ConversationUserRole,
    ) -> impl Future<Output = Result<ConversationUser, AppError>> + Send;

    /// Marks a conversation as read for a user (resets unread_count to 0).
    fn mark_read(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Increments unread_count for all users in a conversation except the sender.
    fn increment_unread(
        &self,
        conversation_id: ConversationId,
        exclude_user_id: UserId,
    ) -> impl Future<Output = Result<Vec<(UserId, i32)>, AppError>> + Send;

    /// Gets the membership row for a user in a conversation.
    fn get(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> impl Future<Output = Result<ConversationUser, AppError>> + Send;

    /// Lists all users in a conversation.
    fn list_by_conversation(
        &self,
        conversation_id: ConversationId,
    ) -> impl Future<Output = Result<Vec<ConversationUser>, AppError>> + Send;

    /// Resets unread_count to 0 for ALL users in a conversation (used by /clear).
    fn reset_all_unread(
        &self,
        conversation_id: ConversationId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;
}
```

- [ ] **Step 5: Add `TagRepo` trait**

```rust
/// Tag operations.
pub trait TagRepo: Send + Sync {
    /// Creates a tag (idempotent — returns existing if name matches).
    fn create_or_get(
        &self,
        input: CreateTag,
    ) -> impl Future<Output = Result<Tag, AppError>> + Send;

    /// Lists all tags for a user.
    fn list_by_user(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<Tag>, AppError>> + Send;

    /// Adds a tag to a conversation.
    fn tag_conversation(
        &self,
        conversation_id: ConversationId,
        tag_id: TagId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Removes a tag from a conversation.
    fn untag_conversation(
        &self,
        conversation_id: ConversationId,
        tag_id: TagId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Adds a tag to a message.
    fn tag_message(
        &self,
        message_id: MessageId,
        tag_id: TagId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Removes a tag from a message.
    fn untag_message(
        &self,
        message_id: MessageId,
        tag_id: TagId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Lists tags for a conversation.
    fn list_by_conversation(
        &self,
        conversation_id: ConversationId,
    ) -> impl Future<Output = Result<Vec<Tag>, AppError>> + Send;
}
```

- [ ] **Step 6: Update imports at top of repo.rs**

Add the new types to the imports: `ConversationUserRole`, `ConversationWithDetails`, `ConversationUser`, `Tag`, `TagId`, `CreateTag`, `ListConversationsFilter`.

- [ ] **Step 7: Build check**

Run: `cd backend && cargo build -p sober-core -q`

- [ ] **Step 8: Commit**

```
feat(core): add repo traits for conversation users, tags, pagination
```

---

## Chunk 2: Database Layer (sober-db)

### Task 5: Update Existing Row Types and Repos

**Files:**
- Modify: `backend/crates/sober-db/src/rows.rs`
- Modify: `backend/crates/sober-db/src/repos/conversations.rs`
- Modify: `backend/crates/sober-db/src/repos/messages.rs`

**Note:** Row types are in `backend/crates/sober-db/src/rows.rs` (top-level module in sober-db, NOT inside repos/).

- [ ] **Step 1: Update `ConversationRow` in rows.rs**

Add `kind` and `is_archived` fields:

```rust
pub(crate) struct ConversationRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: Option<String>,
    pub workspace_id: Option<Uuid>,
    pub kind: ConversationKind,
    pub is_archived: bool,
    pub permission_mode: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

Update the `From<ConversationRow> for Conversation` impl to map the new fields.

- [ ] **Step 2: Update `MessageRow` in rows.rs**

Add `user_id` field:

```rust
pub(crate) struct MessageRow {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub role: MessageRole,
    pub content: String,
    pub tool_calls: Option<serde_json::Value>,
    pub tool_result: Option<serde_json::Value>,
    pub token_count: Option<i32>,
    pub user_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}
```

Update the `From<MessageRow> for Message` impl.

- [ ] **Step 3: Add new row types in rows.rs**

```rust
/// Row type for the conversation_users table.
#[derive(sqlx::FromRow)]
pub(crate) struct ConversationUserRow {
    pub conversation_id: Uuid,
    pub user_id: Uuid,
    pub unread_count: i32,
    pub last_read_at: Option<DateTime<Utc>>,
    pub role: ConversationUserRole,
    pub joined_at: DateTime<Utc>,
}

/// Row type for the tags table.
#[derive(sqlx::FromRow)]
pub(crate) struct TagRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub color: String,
    pub created_at: DateTime<Utc>,
}
```

Add `From` impls for both converting to domain types.

- [ ] **Step 4: Update PgConversationRepo — fix all SELECT queries**

Every query that selects from `conversations` now needs to include `kind` and `is_archived`. Update:
- `create()` — INSERT should return new columns
- `get_by_id()` — SELECT should include new columns
- `list_by_user()` — SELECT should include new columns
- `find_latest_by_user_and_workspace()` — SELECT should include new columns

- [ ] **Step 5: Add `list_with_details()` to PgConversationRepo**

This is the complex query that joins conversations with conversation_users (for unread) and tags:

```rust
async fn list_with_details(
    &self,
    user_id: UserId,
    filter: ListConversationsFilter,
) -> Result<Vec<ConversationWithDetails>, AppError> {
    // Strategy: fetch conversations with unread_count via JOIN,
    // then batch-fetch tags for all returned conversations.
    // Build dynamic WHERE clauses based on filter.
}
```

Use a two-query approach:
1. Query conversations joined with conversation_users for unread_count, filtered by params
2. Query tags for all returned conversation IDs via `conversation_tags JOIN tags`
3. Assemble in Rust

- [ ] **Step 6: Add `get_inbox()` to PgConversationRepo**

```sql
SELECT ... FROM conversations WHERE user_id = $1 AND kind = 'inbox'
```

- [ ] **Step 7: Add `update_archived()` to PgConversationRepo**

```sql
UPDATE conversations SET is_archived = $2 WHERE id = $1
```

Note: do NOT update `updated_at` per design doc.

- [ ] **Step 8: Update PgConversationRepo `create()` to insert conversation_users**

The create method should also insert a `conversation_users` row with `role = 'owner'`. Use a transaction.

- [ ] **Step 9: Update PgConversationRepo `delete()` to block inbox deletion**

Check `kind != 'inbox'` before deleting. Return `AppError::Forbidden` if inbox.

- [ ] **Step 10: Update PgMessageRepo SELECT queries for user_id**

Update `create()` and `list_by_conversation()` to include the `user_id` column.

- [ ] **Step 11: Add `list_paginated()` to PgMessageRepo**

```sql
SELECT ... FROM messages
WHERE conversation_id = $1
  AND ($2::uuid IS NULL OR id < $2)
ORDER BY id DESC
LIMIT $3
```

Return results in chronological order (reverse after fetch).

- [ ] **Step 12: Add `delete()`, `clear_conversation()`, `get_by_id()` to PgMessageRepo**

Simple single-query implementations.

- [ ] **Step 13: Build check**

Run: `cd backend && cargo build -p sober-db -q`

- [ ] **Step 14: Commit**

```
feat(db): update conversation/message repos for new schema, add pagination
```

---

### Task 6: New Repos — ConversationUserRepo and TagRepo

**Files:**
- Create: `backend/crates/sober-db/src/repos/conversation_users.rs`
- Create: `backend/crates/sober-db/src/repos/tags.rs`
- Modify: `backend/crates/sober-db/src/repos/mod.rs`

- [ ] **Step 1: Create `PgConversationUserRepo`**

File: `backend/crates/sober-db/src/repos/conversation_users.rs`

```rust
use sqlx::PgPool;
use sober_core::{
    error::AppError,
    types::{ConversationId, ConversationUser, ConversationUserRole, UserId},
};

pub struct PgConversationUserRepo {
    pool: PgPool,
}

impl PgConversationUserRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
```

Implement all `ConversationUserRepo` trait methods:
- `create`: INSERT into conversation_users, RETURNING all columns
- `mark_read`: UPDATE SET unread_count = 0, last_read_at = now()
- `increment_unread`: UPDATE SET unread_count = unread_count + 1 WHERE user_id != $2, RETURNING user_id, unread_count
- `get`: SELECT by composite PK

- [ ] **Step 2: Create `PgTagRepo`**

File: `backend/crates/sober-db/src/repos/tags.rs`

Implement all `TagRepo` trait methods:
- `create_or_get`: INSERT ON CONFLICT DO NOTHING, then SELECT
- `list_by_user`: SELECT WHERE user_id = $1 ORDER BY name
- `tag_conversation`: INSERT INTO conversation_tags ON CONFLICT DO NOTHING
- `untag_conversation`: DELETE FROM conversation_tags
- `tag_message`: INSERT INTO message_tags ON CONFLICT DO NOTHING
- `untag_message`: DELETE FROM message_tags
- `list_by_conversation`: SELECT tags JOIN conversation_tags

Color assignment: use a fixed palette and assign based on `(tag count for user) % palette.len()`.

```rust
const TAG_COLORS: &[&str] = &[
    "#ef4444", "#f97316", "#eab308", "#22c55e",
    "#06b6d4", "#3b82f6", "#8b5cf6", "#ec4899",
];
```

- [ ] **Step 3: Register new repos in mod.rs**

Add `pub mod conversation_users;` and `pub mod tags;` to `backend/crates/sober-db/src/repos/mod.rs`. Re-export the structs.

- [ ] **Step 4: Build check**

Run: `cd backend && cargo build -p sober-db -q`

- [ ] **Step 5: Commit**

```
feat(db): add PgConversationUserRepo and PgTagRepo implementations
```

---

## Chunk 3: API Layer

### Task 7: Update AppState and Existing Conversation Routes

**Files:**
- Modify: `backend/crates/sober-api/src/state.rs`
- Modify: `backend/crates/sober-api/src/routes/conversations.rs`

**Note on repo pattern:** This codebase constructs repos per-request from the PgPool, NOT stored in AppState. Every handler does `let repo = PgConversationRepo::new(state.db.clone())`. Follow this pattern for all new repos: `let cu_repo = PgConversationUserRepo::new(state.db.clone())`, `let tag_repo = PgTagRepo::new(state.db.clone())`.

- [ ] **Step 1: Update `list_conversations` handler**

Accept query params: `archived`, `kind`, `tag`, `search`. Call `list_with_details()` instead of `list_by_user()`. Return enriched response with `unread_count`, `kind`, `is_archived`, `tags[]`.

- [ ] **Step 3: Update `get_conversation` handler**

Remove inline messages from response. Return conversation with `unread_count`, `kind`, `is_archived`, `tags[]`, `users[]`. Client will fetch messages separately via the paginated endpoint.

- [ ] **Step 4: Update `create_conversation` handler**

Set `kind = 'direct'`. The repo now creates a `conversation_users` row automatically (Task 5, Step 8).

- [ ] **Step 5: Update `delete_conversation` handler**

Return 403 if conversation `kind == 'inbox'`.

- [ ] **Step 6: Update `update_conversation` handler**

Accept optional `archived` field in the PATCH body. Call `update_archived()` when present.

- [ ] **Step 7: Add `get_inbox` handler**

```rust
async fn get_inbox(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
) -> Result<impl IntoResponse, AppError> {
    let repo = PgConversationRepo::new(state.db.clone());
    let conv = repo.get_inbox(auth_user.user_id).await?;
    Ok(ApiResponse::new(conv))
}
```

Route: `GET /conversations/inbox` (must be registered BEFORE `/:id` to avoid conflict).

- [ ] **Step 8: Add `mark_read` handler**

Route: `POST /conversations/:id/read`

```rust
async fn mark_read(...) -> Result<impl IntoResponse, AppError> {
    // Verify ownership via conversation_users.get()
    let cu_repo = PgConversationUserRepo::new(state.db.clone());
    cu_repo.mark_read(id, auth_user.user_id).await?;
    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}
```

- [ ] **Step 9: Add `clear_messages` handler**

Route: `DELETE /conversations/:id/messages`

```rust
async fn clear_messages(...) -> Result<impl IntoResponse, AppError> {
    // Verify ownership
    let msg_repo = PgMessageRepo::new(state.db.clone());
    let cu_repo = PgConversationUserRepo::new(state.db.clone());
    msg_repo.clear_conversation(id).await?;
    // Reset unread for ALL users in this conversation (design section 1.9)
    cu_repo.reset_all_unread(id).await?;
    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}
```

- [ ] **Step 10: Update route registration**

Add new routes to the router. Ensure `/conversations/inbox` is before `/conversations/:id`.

- [ ] **Step 11: Build check**

Run: `cd backend && cargo build -p sober-api -q`

- [ ] **Step 12: Commit**

```
feat(api): update conversation endpoints with inbox, read, archive, clear
```

---

### Task 8: Message Routes (Pagination + Deletion)

**Files:**
- Create: `backend/crates/sober-api/src/routes/messages.rs`
- Modify: `backend/crates/sober-api/src/routes/mod.rs`
- Modify: `backend/crates/sober-api/src/lib.rs`

- [ ] **Step 1: Create messages route module**

```rust
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/conversations/{id}/messages", get(list_messages))
        .route("/messages/{id}", delete(delete_message))
        .route("/messages/{id}/tags", post(add_message_tag))
        .route("/messages/{id}/tags/{tag_id}", delete(remove_message_tag))
}
```

- [ ] **Step 2: Implement `list_messages` handler**

Query params: `before` (UUID cursor, optional), `limit` (default 50, max 100).

```rust
async fn list_messages(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<ConversationId>,
    Query(params): Query<PaginationParams>,
) -> Result<impl IntoResponse, AppError> {
    let conv_repo = PgConversationRepo::new(state.db.clone());
    let msg_repo = PgMessageRepo::new(state.db.clone());

    // Verify conversation ownership
    let conv = conv_repo.get_by_id(id).await?;
    if conv.user_id != auth_user.user_id { return Err(AppError::NotFound(...)); }

    let limit = params.limit.unwrap_or(50).min(100);
    let messages = msg_repo.list_paginated(id, params.before, limit).await?;
    Ok(ApiResponse::new(messages))
}
```

- [ ] **Step 3: Implement `delete_message` handler**

Authorization: conversation owner OR message sender (`messages.user_id`).

```rust
async fn delete_message(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<MessageId>,
) -> Result<impl IntoResponse, AppError> {
    let msg_repo = PgMessageRepo::new(state.db.clone());
    let conv_repo = PgConversationRepo::new(state.db.clone());

    let msg = msg_repo.get_by_id(id).await?;
    let conv = conv_repo.get_by_id(msg.conversation_id).await?;

    let is_owner = conv.user_id == auth_user.user_id;
    let is_sender = msg.user_id == Some(auth_user.user_id);
    if !is_owner && !is_sender {
        return Err(AppError::NotFound("message not found".into()));
    }

    msg_repo.delete(id).await?;
    Ok(ApiResponse::new(serde_json::json!({"deleted": true})))
}
```

- [ ] **Step 4: Implement message tag handlers**

`add_message_tag` and `remove_message_tag` — verify conversation ownership, then delegate to `TagRepo`.

- [ ] **Step 5: Register routes**

Add `messages::routes()` to the main router in `lib.rs`.

- [ ] **Step 6: Build check**

Run: `cd backend && cargo build -p sober-api -q`

- [ ] **Step 7: Commit**

```
feat(api): add paginated messages, message deletion, message tags endpoints
```

---

### Task 9: Tag Routes

**Files:**
- Create: `backend/crates/sober-api/src/routes/tags.rs`

- [ ] **Step 1: Create tags route module**

```rust
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tags", get(list_tags))
        .route("/conversations/{id}/tags", post(add_conversation_tag))
        .route("/conversations/{id}/tags/{tag_id}", delete(remove_conversation_tag))
}
```

- [ ] **Step 2: Implement `list_tags` handler**

Returns all tags for the authenticated user (for autocomplete).

- [ ] **Step 3: Implement `add_conversation_tag` handler**

Accepts `{ "name": "string" }`. Creates tag if new (idempotent), then adds junction row.

```rust
#[derive(Deserialize)]
struct AddTagRequest {
    name: String,
}

async fn add_conversation_tag(...) -> Result<impl IntoResponse, AppError> {
    // Verify conversation ownership
    // Create or get tag (auto-assign color)
    let tag = state.tags.create_or_get(CreateTag {
        user_id: auth_user.user_id,
        name: req.name,
        color: /* auto from palette */,
    }).await?;
    state.tags.tag_conversation(id, tag.id).await?;
    Ok(ApiResponse::new(tag))
}
```

- [ ] **Step 4: Implement `remove_conversation_tag` handler**

Verify ownership, then remove junction row.

- [ ] **Step 5: Register in router**

- [ ] **Step 6: Build check**

Run: `cd backend && cargo build -p sober-api -q`

- [ ] **Step 7: Commit**

```
feat(api): add tag CRUD and conversation tagging endpoints
```

---

### Task 10: WebSocket — UserConnectionRegistry and Unread Events

**Files:**
- Modify: `backend/crates/sober-api/src/connections.rs`
- Modify: `backend/crates/sober-api/src/routes/ws.rs`
- Modify: `backend/crates/sober-api/src/state.rs`
- Modify: `backend/crates/sober-api/src/subscribe.rs`

- [ ] **Step 1: Add `UserConnectionRegistry` to connections.rs**

```rust
/// Tracks WebSocket connections per user for cross-conversation events.
#[derive(Clone, Default)]
pub struct UserConnectionRegistry {
    inner: Arc<RwLock<HashMap<String, Vec<mpsc::Sender<ServerWsMessage>>>>>,
}

impl UserConnectionRegistry {
    pub fn new() -> Self { Self::default() }

    pub async fn register(&self, user_id: &str, sender: mpsc::Sender<ServerWsMessage>) { ... }
    /// Prunes closed senders for a user (same pattern as ConnectionRegistry).
    /// Uses `retain(!is_closed())` to remove only dead senders, not all senders
    /// (a user may have multiple tabs/connections open).
    pub async fn unregister(&self, user_id: &str) { ... }
    pub async fn send(&self, user_id: &str, msg: ServerWsMessage) { ... }
}
```

- [ ] **Step 2: Add `chat.unread` to `ServerWsMessage` enum in ws.rs**

```rust
ChatUnread {
    conversation_id: String,
    unread_count: i32,
},
```

Serializes as `{ "type": "chat.unread", "conversation_id": "...", "unread_count": N }`.

- [ ] **Step 3: Register user connection on WebSocket open**

In the WebSocket handler, after auth, register the sender in `UserConnectionRegistry`. Unregister on close.

- [ ] **Step 4: Mark as read on `chat.subscribe`**

When the client sends `chat.subscribe`, call `conversation_users.mark_read()` for the user.

- [ ] **Step 5: Wire unread notifications in subscribe.rs**

When a `NewMessage` event arrives from the agent subscription:
1. Increment unread for all users except sender via `conversation_users.increment_unread()`
2. For each affected user, send `chat.unread` via `UserConnectionRegistry`
3. Skip users who have an active subscription to that conversation (they're viewing it)

- [ ] **Step 6: Add `UserConnectionRegistry` to AppState**

`UserConnectionRegistry` (like the existing `ConnectionRegistry`) belongs in AppState since it's cross-request shared state, not a per-request repo.

- [ ] **Step 7: Build check**

Run: `cd backend && cargo build -p sober-api -q`

- [ ] **Step 8: Run full backend tests**

Run: `cd backend && cargo test --workspace -q`

- [ ] **Step 9: Run clippy on full workspace**

Run: `cd backend && cargo clippy -q -- -D warnings`

- [ ] **Step 10: Commit**

```
feat(api): add UserConnectionRegistry and chat.unread WebSocket events
```

---

### Task 10b: Inbox Creation on User Registration

**Files:**
- Modify: `backend/crates/sober-api/src/routes/auth.rs`

The design states: "Inbox: one per user, created on user registration." The migration backfills existing users, but new registrations must also create an inbox.

- [ ] **Step 1: Modify the registration handler**

After the user is successfully created in the `register` handler, create an inbox conversation and conversation_users row:

```rust
// After user creation succeeds:
let conv_repo = PgConversationRepo::new(state.db.clone());
let cu_repo = PgConversationUserRepo::new(state.db.clone());

// Create inbox conversation (kind = 'inbox')
let inbox = conv_repo.create_inbox(user.id).await?;
cu_repo.create(inbox.id, user.id, ConversationUserRole::Owner).await?;
```

This requires adding a `create_inbox` method to `ConversationRepo` trait and `PgConversationRepo`:

```rust
/// Creates an inbox conversation for a user.
fn create_inbox(
    &self,
    user_id: UserId,
) -> impl Future<Output = Result<Conversation, AppError>> + Send;
```

The SQL: `INSERT INTO conversations (id, user_id, kind, created_at, updated_at) VALUES ($1, $2, 'inbox', now(), now()) RETURNING ...`

- [ ] **Step 2: Build check**

Run: `cd backend && cargo build -p sober-api -q`

- [ ] **Step 3: Commit**

```
feat(api): create inbox conversation on user registration
```

---

## Chunk 4: Frontend — Types, Services, Stores

### Task 11: Frontend Types and Services

**Files:**
- Modify: `frontend/src/lib/types/index.ts`
- Modify: `frontend/src/lib/services/conversations.ts`
- Create: `frontend/src/lib/services/tags.ts`

- [ ] **Step 1: Update TypeScript types**

```typescript
export type ConversationKind = 'direct' | 'group' | 'inbox';
export type ConversationUserRole = 'owner' | 'member';

export interface Conversation {
    id: string;
    title: string | null;
    workspace_id?: string;
    kind: ConversationKind;
    is_archived: boolean;
    permission_mode: PermissionMode;
    unread_count: number;
    tags: Tag[];
    created_at: string;
    updated_at: string;
}

export interface Message {
    id: string;
    role: 'User' | 'Assistant' | 'System' | 'Tool';
    content: string;
    tool_calls?: unknown;
    tool_result?: unknown;
    token_count: number;
    user_id?: string;
    created_at: string;
}

export interface Tag {
    id: string;
    name: string;
    color: string;
    created_at: string;
}

export interface ConversationUser {
    conversation_id: string;
    user_id: string;
    unread_count: number;
    role: ConversationUserRole;
    joined_at: string;
}
```

Remove `ConversationWithMessages` interface.

- [ ] **Step 2: Update conversation service**

```typescript
export const conversationService = {
    list: (params?: { archived?: boolean; kind?: string; tag?: string; search?: string }) =>
        api<Conversation[]>('/conversations', { params }),
    get: (id: string) => api<Conversation>(`/conversations/${id}`),
    create: () => api<Conversation>('/conversations', { method: 'POST' }),
    updateTitle: (id: string, title: string) =>
        api<{ id: string; title: string }>(`/conversations/${id}`, {
            method: 'PATCH', body: { title },
        }),
    updatePermissionMode: (id: string, mode: PermissionMode) =>
        api<{ id: string; permission_mode: PermissionMode }>(`/conversations/${id}`, {
            method: 'PATCH', body: { permission_mode: mode },
        }),
    archive: (id: string, archived: boolean) =>
        api(`/conversations/${id}`, { method: 'PATCH', body: { archived } }),
    delete: (id: string) => api<{ deleted: boolean }>(`/conversations/${id}`, { method: 'DELETE' }),
    getInbox: () => api<Conversation>('/conversations/inbox'),
    markRead: (id: string) => api('/conversations/' + id + '/read', { method: 'POST' }),
    clearMessages: (id: string) =>
        api(`/conversations/${id}/messages`, { method: 'DELETE' }),
    listMessages: (id: string, before?: string, limit = 50) => {
        const params = new URLSearchParams({ limit: String(limit) });
        if (before) params.set('before', before);
        return api<Message[]>(`/conversations/${id}/messages?${params}`);
    },
    deleteMessage: (id: string) =>
        api<{ deleted: boolean }>(`/messages/${id}`, { method: 'DELETE' }),
};
```

- [ ] **Step 3: Create tag service**

```typescript
import { api } from '$lib/utils/api';
import type { Tag } from '$lib/types';

export const tagService = {
    list: () => api<Tag[]>('/tags'),
    addToConversation: (conversationId: string, name: string) =>
        api<Tag>(`/conversations/${conversationId}/tags`, {
            method: 'POST', body: { name },
        }),
    removeFromConversation: (conversationId: string, tagId: string) =>
        api(`/conversations/${conversationId}/tags/${tagId}`, { method: 'DELETE' }),
    addToMessage: (messageId: string, name: string) =>
        api<Tag>(`/messages/${messageId}/tags`, { method: 'POST', body: { name } }),
    removeFromMessage: (messageId: string, tagId: string) =>
        api(`/messages/${messageId}/tags/${tagId}`, { method: 'DELETE' }),
};
```

- [ ] **Step 4: Build check**

Run: `cd frontend && pnpm check`

- [ ] **Step 5: Commit**

```
feat(frontend): update types and services for conversation improvements
```

---

### Task 12: Frontend Stores

**Files:**
- Modify: `frontend/src/lib/stores/conversations.svelte.ts`
- Modify: `frontend/src/lib/stores/websocket.svelte.ts`

- [ ] **Step 1: Enhance conversations store**

```typescript
export const conversations = (() => {
    let items = $state<Conversation[]>([]);
    let loading = $state(false);
    let showArchived = $state(false);
    let inbox = $state<Conversation | null>(null);

    return {
        get items() { return items; },
        get loading() { return loading; },
        get showArchived() { return showArchived; },
        get inbox() { return inbox; },

        set(list: Conversation[]) { items = list; },
        setLoading(v: boolean) { loading = v; },
        setShowArchived(v: boolean) { showArchived = v; },
        setInbox(conv: Conversation) { inbox = conv; },

        prepend(conv: Conversation) { items = [conv, ...items]; },
        updateTitle(id: string, title: string) {
            items = items.map(c => c.id === id ? { ...c, title } : c);
        },
        remove(id: string) {
            items = items.filter(c => c.id !== id);
        },

        updateUnread(conversationId: string, unreadCount: number) {
            items = items.map(c =>
                c.id === conversationId ? { ...c, unread_count: unreadCount } : c
            );
            // Re-sort: unread first, then by updated_at
            items.sort((a, b) => {
                if (a.unread_count > 0 && b.unread_count === 0) return -1;
                if (a.unread_count === 0 && b.unread_count > 0) return 1;
                return new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime();
            });
        },

        markRead(conversationId: string) {
            items = items.map(c =>
                c.id === conversationId ? { ...c, unread_count: 0 } : c
            );
        },

        archive(id: string) {
            items = items.map(c => c.id === id ? { ...c, is_archived: true } : c);
        },

        unarchive(id: string) {
            items = items.map(c => c.id === id ? { ...c, is_archived: false } : c);
        },
    };
})();
```

- [ ] **Step 2: Handle `chat.unread` in WebSocket store**

Add handler for the new message type in the WebSocket message processing:

```typescript
case 'chat.unread':
    conversations.updateUnread(msg.conversation_id, msg.unread_count);
    break;
```

- [ ] **Step 3: Build check**

Run: `cd frontend && pnpm check`

- [ ] **Step 4: Commit**

```
feat(frontend): enhance stores with unread tracking and archive support
```

---

## Chunk 5: Frontend — Pages and Components

### Task 13: Dashboard Page

**Files:**
- Create: `frontend/src/routes/(app)/+page.ts`
- Create: `frontend/src/routes/(app)/+page.svelte`

- [ ] **Step 1: Create dashboard loader**

```typescript
// +page.ts
import { conversationService } from '$lib/services/conversations';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ fetch }) => {
    const [all, inbox] = await Promise.all([
        conversationService.list(),
        conversationService.getInbox(),
    ]);
    return { conversations: all, inbox };
};
```

- [ ] **Step 2: Create dashboard page**

Build the dashboard with:
- Search bar (filters conversations by title/tags client-side, with server fallback for large lists)
- Unread section (conversations with unread_count > 0)
- Recent conversations (last 20 non-archived)
- Quick actions (new conversation, inbox link)

Key patterns:
- Use `$derived` for filtered/grouped lists
- Use `$state` for search query
- Navigate to `/chat/[id]` on click

- [ ] **Step 3: Build check**

Run: `cd frontend && pnpm check`

- [ ] **Step 4: Commit**

```
feat(frontend): add conversation dashboard page
```

---

### Task 14: Sidebar Enhancements

**Files:**
- Modify: `frontend/src/routes/(app)/+layout.svelte`

- [ ] **Step 1: Add inbox pinned at top**

Always show inbox above conversation list with a distinct icon (e.g., inbox SVG).

- [ ] **Step 2: Add unread badges**

Show pill with unread count next to conversations with `unread_count > 0`. Use `$derived` to compute visibility.

- [ ] **Step 3: Add tag pills**

Show small colored dots next to conversation titles from `conversation.tags`.

- [ ] **Step 4: Add archive toggle**

Button at bottom of sidebar to show/hide archived conversations. Use `conversations.showArchived` state. Filter the displayed list with `$derived`:

```typescript
let visibleConversations = $derived(
    conversations.items.filter(c =>
        c.kind !== 'inbox' &&
        (conversations.showArchived || !c.is_archived)
    )
);
```

- [ ] **Step 5: Add conversation context menu**

"..." button on each conversation in sidebar revealing: Archive/Unarchive, Delete (with confirmation).

- [ ] **Step 6: Build check**

Run: `cd frontend && pnpm check`

- [ ] **Step 7: Commit**

```
feat(frontend): enhance sidebar with inbox, unread badges, tags, archive
```

---

### Task 15: Chat Page — Pagination and Mark Read

**Files:**
- Modify: `frontend/src/routes/(app)/chat/[id]/+page.ts`
- Modify: `frontend/src/routes/(app)/chat/[id]/+page.svelte`

- [ ] **Step 1: Update page loader**

Fetch conversation and first page of messages separately:

```typescript
export const load: PageLoad = async ({ params }) => {
    const [conversation, messages] = await Promise.all([
        conversationService.get(params.id),
        conversationService.listMessages(params.id),
    ]);
    return { conversation, messages };
};
```

- [ ] **Step 2: Implement infinite scroll up**

```typescript
let messages = $state<Message[]>(data.messages);
let loadingMore = $state(false);
let allLoaded = $state(data.messages.length < 50);

async function loadMore() {
    if (loadingMore || allLoaded) return;
    loadingMore = true;
    const oldest = messages[0];
    const older = await conversationService.listMessages(
        data.conversation.id, oldest.id
    );
    if (older.length < 50) allLoaded = true;
    // Preserve scroll position
    const container = messagesContainer;
    const prevHeight = container.scrollHeight;
    messages = [...older, ...messages];
    // After DOM update, restore scroll
    tick().then(() => {
        container.scrollTop = container.scrollHeight - prevHeight;
    });
    loadingMore = false;
}
```

Trigger `loadMore()` via scroll event or `IntersectionObserver` at the top of the messages container.

- [ ] **Step 3: Mark as read on load**

The `chat.subscribe` WebSocket message already triggers mark-read on the backend (Task 10, Step 4). On the frontend, also call `conversations.markRead(id)` to immediately clear the sidebar badge.

- [ ] **Step 4: Build check**

Run: `cd frontend && pnpm check`

- [ ] **Step 5: Commit**

```
feat(frontend): add cursor-based message pagination with infinite scroll
```

---

### Task 16: Chat Page — Tags

**Files:**
- Modify: `frontend/src/routes/(app)/chat/[id]/+page.svelte`
- Create: `frontend/src/lib/components/TagInput.svelte`

- [ ] **Step 1: Create `TagInput` component**

Props:
```typescript
interface Props {
    tags: Tag[];
    onAdd: (name: string) => void;
    onRemove: (tagId: string) => void;
}
```

Features:
- Input field with autocomplete dropdown from existing user tags
- Shows current tags as colored pills with "x" to remove
- Enter creates/adds tag
- Fetches user tags on mount via `tagService.list()`

- [ ] **Step 2: Integrate tags in chat page header**

Below the conversation title, show `TagInput` bound to the conversation's tags. Wire `onAdd` to `tagService.addToConversation()` and `onRemove` to `tagService.removeFromConversation()`.

- [ ] **Step 3: Build check**

Run: `cd frontend && pnpm check`

- [ ] **Step 4: Commit**

```
feat(frontend): add conversation tag management with autocomplete
```

---

### Task 17: Chat Page — Message Deletion and Slash Commands

**Files:**
- Modify: `frontend/src/routes/(app)/chat/[id]/+page.svelte`
- Create: `frontend/src/lib/components/SlashCommandPalette.svelte`
- Create: `frontend/src/lib/components/ConfirmDialog.svelte`

- [ ] **Step 1: Create `ConfirmDialog` component**

```typescript
interface Props {
    open: boolean;
    title: string;
    message: string;
    confirmText?: string;
    destructive?: boolean;
    onConfirm: () => void;
    onCancel: () => void;
}
```

Modal overlay with cancel/confirm buttons. `destructive` styles the confirm button red.

- [ ] **Step 2: Add message deletion**

On each message, show a delete button on hover (or in a context menu). Only show for messages the user can delete (conversation owner or message sender).

On click → show `ConfirmDialog` → on confirm, call `api<{deleted: boolean}>(\`/messages/${id}\`, { method: 'DELETE' })` → remove from local `messages` array.

- [ ] **Step 3: Create `SlashCommandPalette` component**

Shows when input starts with `/`. Available commands:
- `/help` — show available commands (render as ephemeral message)
- `/info` — show conversation metadata (kind, created, message count, tags)
- `/clear` — show confirmation dialog, then call `conversationService.clearMessages(id)`, reset local messages array

```typescript
interface Props {
    query: string;
    conversation: Conversation;
    onExecute: (command: string) => void;
    onClose: () => void;
}
```

- [ ] **Step 4: Integrate slash commands in chat input**

Detect `/` prefix in the message input. Show palette as overlay above input. Execute command on selection. Prevent sending slash commands to the agent.

- [ ] **Step 5: Build check**

Run: `cd frontend && pnpm check`

- [ ] **Step 6: Commit**

```
feat(frontend): add message deletion, slash commands, confirm dialog
```

---

### Task 18: Archive and Delete UI

**Files:**
- Modify: `frontend/src/routes/(app)/chat/[id]/+page.svelte`
- Modify: `frontend/src/routes/(app)/+layout.svelte`

- [ ] **Step 1: Add archive/delete to chat page header**

Add a dropdown menu or icon buttons in the chat page header:
- Archive/Unarchive toggle (calls `conversationService.archive(id, !is_archived)`)
- Delete button (shows ConfirmDialog, blocked for inbox, navigates to `/` after delete)

- [ ] **Step 2: Wire archive/delete to conversations store**

After successful API call, update the store (`conversations.archive()`, `conversations.remove()`).

- [ ] **Step 3: Build check**

Run: `cd frontend && pnpm check`

- [ ] **Step 4: Run full frontend tests**

Run: `cd frontend && pnpm test --silent`

- [ ] **Step 5: Commit**

```
feat(frontend): add archive and delete UI for conversations
```

---

## Chunk 6: Integration, sqlx Prepare, and Final Verification

### Task 19: Integration and Cleanup

**Files:**
- Various

- [ ] **Step 1: Run full backend build**

Run: `cd backend && cargo build -q`

- [ ] **Step 2: Run full backend tests**

Run: `cd backend && cargo test --workspace -q`

Fix any compilation errors or test failures.

- [ ] **Step 3: Run clippy**

Run: `cd backend && cargo clippy -q -- -D warnings`

- [ ] **Step 4: Run sqlx prepare for offline mode**

Run: `cd backend && cargo sqlx prepare --workspace`

Commit the updated `.sqlx/` directory.

- [ ] **Step 5: Run frontend checks**

Run: `cd frontend && pnpm check && pnpm test --silent`

- [ ] **Step 6: Move plan folder to done/**

Per CLAUDE.md: move from `active/` to `done/` in the last commit.

```bash
git mv docs/plans/active/025-conversation-improvements docs/plans/done/025-conversation-improvements
```

- [ ] **Step 7: Version bump**

Bump MINOR version in all affected crate `Cargo.toml` files (feat/ branch = MINOR bump):
- `sober-core`
- `sober-db`
- `sober-api`

- [ ] **Step 8: Final commit**

```
feat(025): conversation improvements — integration and version bump
```
