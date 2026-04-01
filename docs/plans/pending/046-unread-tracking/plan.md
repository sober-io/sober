# #046 Centralized Unread Tracking — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every message stored in a conversation correctly increments unread counts for non-viewing members, with a `last_read_message_id` cursor for "unread from here" dividers.

**Architecture:** Move unread increment into `PgMessageRepo::create` so every message — human, assistant, scheduler — goes through one path. Add `last_read_message_id` to `conversation_users` for precise read position tracking. Frontend renders a divider after the last-read message and resets it via IntersectionObserver when the divider scrolls into view.

**Tech Stack:** PostgreSQL migration, Rust (sober-core, sober-db, sober-api, sober-agent), Svelte 5, TypeScript

---

## File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `backend/migrations/20260401000001_unread_last_read_message.sql` | Add `last_read_message_id` column |
| Modify | `backend/crates/sober-core/src/types/domain.rs:149-162` | Add `last_read_message_id` to `ConversationUser` |
| Modify | `backend/crates/sober-core/src/types/domain.rs:166-181` | Add `last_read_message_id` to `ConversationUserWithUsername` |
| Modify | `backend/crates/sober-core/src/types/domain.rs:200-215` | Add `last_read_message_id` to `ConversationWithDetails` |
| Modify | `backend/crates/sober-core/src/types/repo.rs:277-289` | Update `mark_read` signature, update `increment_unread` signature |
| Modify | `backend/crates/sober-db/src/rows.rs:549-556` | Add `last_read_message_id` to `ConversationUserRow` |
| Modify | `backend/crates/sober-db/src/rows.rs:558-568` | Update `From<ConversationUserRow>` |
| Modify | `backend/crates/sober-db/src/rows.rs:573-593` | Add `last_read_message_id` to `ConversationUserWithUsernameRow` + From |
| Modify | `backend/crates/sober-db/src/rows.rs:632-645` | Add `last_read_message_id` to `ConversationWithUnreadRow` |
| Modify | `backend/crates/sober-db/src/repos/conversation_users.rs:49-66` | Update `mark_read` impl |
| Modify | `backend/crates/sober-db/src/repos/conversation_users.rs:68-95` | Update `increment_unread` impl |
| Modify | `backend/crates/sober-db/src/repos/messages.rs:30-52` | Add unread increment to `create` |
| Modify | `backend/crates/sober-db/src/repos/conversations.rs:180-184` | Add `last_read_message_id` to list query |
| Modify | `backend/crates/sober-api/src/subscribe.rs:62-74` | Remove `handle_new_message_unread` call, push WS notification from new source |
| Modify | `backend/crates/sober-api/src/subscribe.rs:100-137` | Remove `handle_new_message_unread` function |
| Modify | `backend/crates/sober-api/src/routes/conversations.rs:436-452` | Update `mark_read` handler to accept `message_id` |
| Modify | `backend/crates/sober-api/src/routes/ws.rs:358-363` | Update `chat.subscribe` mark_read to pass latest message ID |
| Modify | `backend/crates/sober-agent/src/conversation.rs:257-277` | Store user messages for all triggers (not just Human) |
| Modify | `backend/crates/sober-agent/src/turn.rs:437` | Remove `user_id` from assistant messages |
| Modify | `frontend/src/lib/types/index.ts:39-52` | Add `last_read_message_id` to `Conversation` |
| Modify | `frontend/src/lib/services/conversations.ts:46` | Update `markRead` to accept `messageId` param |
| Modify | `frontend/src/routes/(app)/chat/[id]/+page.svelte:825-839` | Add unread divider rendering |
| Modify | `frontend/src/routes/(app)/chat/[id]/+page.svelte:464-491` | Update `chat.new_message` mark-read to pass message ID |
| Modify | `frontend/src/routes/(app)/chat/[id]/+page.svelte:535-539` | Update `chat.done` mark-read to pass message ID |

---

### Task 1: Database Migration — Add `last_read_message_id`

**Files:**
- Create: `backend/migrations/20260401000001_unread_last_read_message.sql`

- [ ] **Step 1: Write the migration**

```sql
-- Add last_read_message_id to track exact read position.
ALTER TABLE conversation_users
  ADD COLUMN last_read_message_id UUID REFERENCES conversation_messages(id) ON DELETE SET NULL;

-- Backfill: set last_read_message_id to the latest message for users with 0 unread.
UPDATE conversation_users cu
SET last_read_message_id = (
    SELECT id FROM conversation_messages cm
    WHERE cm.conversation_id = cu.conversation_id
    ORDER BY cm.id DESC
    LIMIT 1
)
WHERE cu.unread_count = 0;

-- Index for the FK and for the increment query that filters on this column.
CREATE INDEX idx_conversation_users_last_read_msg
  ON conversation_users (conversation_id, last_read_message_id);
```

- [ ] **Step 2: Run the migration**

```bash
cd backend && cargo run -q --bin sober -- migrate run
```

- [ ] **Step 3: Regenerate sqlx offline data**

```bash
cd backend && cargo sqlx prepare --workspace -q
```

- [ ] **Step 4: Commit**

```bash
git add backend/migrations/20260401000001_unread_last_read_message.sql backend/.sqlx/
git commit -m "feat(db): add last_read_message_id to conversation_users"
```

---

### Task 2: Domain Types — Add `last_read_message_id`

**Files:**
- Modify: `backend/crates/sober-core/src/types/domain.rs`

- [ ] **Step 1: Add field to `ConversationUser`**

In `ConversationUser` (around line 149), add after the `last_read_at` field:

```rust
    /// ID of the last message the user has read. Messages after this are unread.
    pub last_read_message_id: Option<MessageId>,
```

- [ ] **Step 2: Add field to `ConversationUserWithUsername`**

In `ConversationUserWithUsername` (around line 166), add after the `last_read_at` field:

```rust
    /// ID of the last message the user has read.
    pub last_read_message_id: Option<MessageId>,
```

- [ ] **Step 3: Add field to `ConversationWithDetails`**

In `ConversationWithDetails` (around line 200), add after `unread_count`:

```rust
    /// ID of the last message the requesting user has read (for unread divider).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_read_message_id: Option<MessageId>,
```

- [ ] **Step 4: Build to check for compile errors**

```bash
cd backend && cargo check -q -p sober-core 2>&1
```

Expected: compile errors in `sober-db` (row conversion mismatches). That's correct — Task 3 fixes them.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-core/src/types/domain.rs
git commit -m "feat(core): add last_read_message_id to ConversationUser and ConversationWithDetails"
```

---

### Task 3: Row Types — Update sqlx Row Structs and Conversions

**Files:**
- Modify: `backend/crates/sober-db/src/rows.rs`

- [ ] **Step 1: Update `ConversationUserRow`**

Add field to `ConversationUserRow` (around line 549):

```rust
    pub last_read_message_id: Option<Uuid>,
```

- [ ] **Step 2: Update `From<ConversationUserRow> for ConversationUser`**

In the `From` impl (around line 558), add:

```rust
            last_read_message_id: row.last_read_message_id.map(MessageId::from_uuid),
```

- [ ] **Step 3: Update `ConversationUserWithUsernameRow`**

Add field (around line 573):

```rust
    pub last_read_message_id: Option<Uuid>,
```

And in its `From` impl, add the same conversion:

```rust
            last_read_message_id: row.last_read_message_id.map(MessageId::from_uuid),
```

- [ ] **Step 4: Update `ConversationWithUnreadRow`**

Add field (around line 632):

```rust
    pub last_read_message_id: Option<Uuid>,
```

- [ ] **Step 5: Build to verify**

```bash
cd backend && cargo check -q -p sober-db 2>&1
```

Expected: errors in repos and API crates (they need to use the new fields). That's expected — Tasks 4-6 fix them.

- [ ] **Step 6: Commit**

```bash
git add backend/crates/sober-db/src/rows.rs
git commit -m "feat(db): add last_read_message_id to row types and From conversions"
```

---

### Task 4: Repo Layer — Update `conversation_users` Queries

**Files:**
- Modify: `backend/crates/sober-core/src/types/repo.rs`
- Modify: `backend/crates/sober-db/src/repos/conversation_users.rs`

- [ ] **Step 1: Update `ConversationUserRepo` trait — `mark_read` signature**

In `repo.rs` (around line 277), change `mark_read` to accept a message ID:

```rust
    /// Marks a conversation as read up to a given message for a user.
    fn mark_read(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        last_read_message_id: MessageId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;
```

- [ ] **Step 2: Update `ConversationUserRepo` trait — `increment_unread` signature**

Change `increment_unread` (around line 284) to remove the `exclude_user_id` parameter — the query will use `last_read_message_id` instead:

```rust
    /// Increments unread_count for all users whose last_read_message_id is
    /// before the new message (or NULL), excluding the message author.
    /// Returns affected user IDs and new counts.
    fn increment_unread(
        &self,
        conversation_id: ConversationId,
        message_id: MessageId,
        author_user_id: Option<UserId>,
    ) -> impl Future<Output = Result<Vec<(UserId, i32)>, AppError>> + Send;
```

- [ ] **Step 3: Update `PgConversationUserRepo::mark_read` implementation**

In `conversation_users.rs` (around line 49), replace the implementation:

```rust
    async fn mark_read(
        &self,
        conversation_id: ConversationId,
        user_id: UserId,
        last_read_message_id: MessageId,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE conversation_users \
             SET unread_count = 0, last_read_at = now(), last_read_message_id = $3 \
             WHERE conversation_id = $1 AND user_id = $2",
        )
        .bind(conversation_id.as_uuid())
        .bind(user_id.as_uuid())
        .bind(last_read_message_id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(())
    }
```

- [ ] **Step 4: Update `PgConversationUserRepo::increment_unread` implementation**

Replace the implementation (around line 68):

```rust
    async fn increment_unread(
        &self,
        conversation_id: ConversationId,
        message_id: MessageId,
        author_user_id: Option<UserId>,
    ) -> Result<Vec<(UserId, i32)>, AppError> {
        #[derive(sqlx::FromRow)]
        struct UnreadRow {
            user_id: Uuid,
            unread_count: i32,
        }

        // Exclude the message author — their own messages should not count
        // as unread for themselves. Use nil UUID when there is no author
        // (system messages) so the exclusion matches nobody.
        let exclude_id = author_user_id
            .map(|id| *id.as_uuid())
            .unwrap_or(uuid::Uuid::nil());

        let rows = sqlx::query_as::<_, UnreadRow>(
            "UPDATE conversation_users \
             SET unread_count = unread_count + 1 \
             WHERE conversation_id = $1 \
               AND user_id != $3 \
               AND (last_read_message_id IS NULL OR last_read_message_id < $2) \
             RETURNING user_id, unread_count",
        )
        .bind(conversation_id.as_uuid())
        .bind(message_id.as_uuid())
        .bind(exclude_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(rows
            .into_iter()
            .map(|r| (UserId::from_uuid(r.user_id), r.unread_count))
            .collect())
    }
```

- [ ] **Step 5: Update `list_by_conversation` and `list_collaborators` queries**

Both SELECT queries need to include the new column. In `list_by_conversation` (around line 117), update the query string to include `last_read_message_id`:

```rust
        let rows = sqlx::query_as::<_, ConversationUserRow>(
            "SELECT conversation_id, user_id, unread_count, last_read_at, last_read_message_id, role, joined_at \
             FROM conversation_users \
             WHERE conversation_id = $1",
        )
```

In `list_collaborators` (around line 144), update:

```rust
        let rows = sqlx::query_as::<_, ConversationUserWithUsernameRow>(
            "SELECT cu.conversation_id, cu.user_id, u.username, \
             cu.unread_count, cu.last_read_at, cu.last_read_message_id, cu.role, cu.joined_at \
             FROM conversation_users cu \
             JOIN users u ON cu.user_id = u.id \
             WHERE cu.conversation_id = $1 \
             ORDER BY cu.joined_at",
        )
```

Also update the `create` query (around line 34) to include `last_read_message_id` in the RETURNING clause:

```rust
            "INSERT INTO conversation_users (conversation_id, user_id, role) \
             VALUES ($1, $2, $3) \
             RETURNING conversation_id, user_id, unread_count, last_read_at, last_read_message_id, role, joined_at",
```

And the `get` query (around line 97):

```rust
            "SELECT conversation_id, user_id, unread_count, last_read_at, last_read_message_id, role, joined_at \
             FROM conversation_users \
             WHERE conversation_id = $1 AND user_id = $2",
```

- [ ] **Step 6: Build to verify**

```bash
cd backend && cargo check -q -p sober-db 2>&1
```

Expected: errors in `sober-api` and `sober-agent` where `mark_read` / `increment_unread` are called with old signatures. That's expected — Tasks 5-6 fix them.

- [ ] **Step 7: Commit**

```bash
git add backend/crates/sober-core/src/types/repo.rs backend/crates/sober-db/src/repos/conversation_users.rs
git commit -m "feat(db): update mark_read and increment_unread for last_read_message_id"
```

---

### Task 5: Message Repo — Centralize Unread Increment in `create`

**Files:**
- Modify: `backend/crates/sober-db/src/repos/messages.rs`

- [ ] **Step 1: Add unread increment to `PgMessageRepo::create`**

After the INSERT query succeeds and before the `Ok(row.into())` return (around line 50), add a second query to increment unread for all conversation members whose read position is before this new message:

```rust
    async fn create(&self, input: CreateMessage) -> Result<Message, AppError> {
        let id = Uuid::now_v7();
        let row = sqlx::query_as::<_, MessageRow>(
            &format!(
                "INSERT INTO conversation_messages (id, conversation_id, role, content, reasoning, token_count, metadata, user_id) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
                 RETURNING {MSG_COLUMNS}"
            ),
        )
        .bind(id)
        .bind(input.conversation_id.as_uuid())
        .bind(input.role)
        .bind(&input.content)
        .bind(&input.reasoning)
        .bind(input.token_count)
        .bind(&input.metadata)
        .bind(input.user_id.map(|u| *u.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        // Increment unread for all conversation members whose read position
        // is before this message (or NULL), excluding the message author.
        // This is the single centralized path for unread tracking — every
        // stored message goes through here.
        let exclude_author = input.user_id.map(|u| *u.as_uuid()).unwrap_or(Uuid::nil());
        sqlx::query(
            "UPDATE conversation_users \
             SET unread_count = unread_count + 1 \
             WHERE conversation_id = $1 \
               AND user_id != $3 \
               AND (last_read_message_id IS NULL OR last_read_message_id < $2)",
        )
        .bind(input.conversation_id.as_uuid())
        .bind(id)
        .bind(exclude_author)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.into())
    }
```

- [ ] **Step 2: Build to verify**

```bash
cd backend && cargo check -q -p sober-db 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-db/src/repos/messages.rs
git commit -m "feat(db): centralize unread increment in MessageRepo::create"
```

---

### Task 6: API Layer — Update Subscribe, Mark-Read, and List Queries

**Files:**
- Modify: `backend/crates/sober-api/src/subscribe.rs`
- Modify: `backend/crates/sober-api/src/routes/conversations.rs`
- Modify: `backend/crates/sober-api/src/routes/ws.rs`
- Modify: `backend/crates/sober-db/src/repos/conversations.rs`

- [ ] **Step 1: Remove `handle_new_message_unread` from `subscribe.rs`**

Delete the `handle_new_message_unread` function entirely (lines 100-137), and remove the call to it in the subscription loop (lines 63-74). The unread increment now happens in the message repo.

However, we still need to **push `chat.unread` WS notifications** for live updates. Replace the old call with a new approach: when a `NewMessage` event arrives, query the current unread counts for that conversation's members and push `chat.unread` to each connected user.

Replace lines 62-74 with:

```rust
                            // Push live unread notifications for NewMessage events.
                            // The unread count was already incremented by MessageRepo::create;
                            // we just need to notify connected users.
                            if let Some(proto::conversation_update::Event::NewMessage(_)) =
                                update.event
                            {
                                push_unread_notifications(
                                    &conversation_id,
                                    &db,
                                    &user_connections,
                                )
                                .await;
                            }
```

Then replace the old `handle_new_message_unread` function with:

```rust
/// Pushes current unread counts to connected users for a conversation.
///
/// Called after a `NewMessage` event — the message repo already incremented
/// counts, so we just read and push.
async fn push_unread_notifications(
    conversation_id: &str,
    db: &PgPool,
    user_connections: &UserConnectionRegistry,
) {
    let Ok(conv_uuid) = conversation_id.parse::<uuid::Uuid>() else {
        return;
    };
    let conv_id = ConversationId::from_uuid(conv_uuid);

    let cu_repo = PgConversationUserRepo::new(db.clone());
    let Ok(members) = cu_repo.list_by_conversation(conv_id).await else {
        return;
    };

    for member in members {
        if member.unread_count > 0 {
            user_connections
                .send(
                    &member.user_id.to_string(),
                    ServerWsMessage::ChatUnread {
                        conversation_id: conversation_id.to_string(),
                        unread_count: member.unread_count,
                    },
                )
                .await;
        }
    }
}
```

- [ ] **Step 2: Update `mark_read` route handler**

In `conversations.rs` (around line 436), update the handler to accept an optional `message_id` from the request body. If not provided, query the latest message ID:

```rust
#[derive(Deserialize)]
struct MarkReadRequest {
    /// The ID of the last message the user has seen. If omitted, uses the
    /// latest message in the conversation.
    message_id: Option<uuid::Uuid>,
}

/// `POST /api/v1/conversations/:id/read` — mark conversation as read.
async fn mark_read(
    State(state): State<Arc<AppState>>,
    auth_user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    body: Option<axum::Json<MarkReadRequest>>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let conversation_id = ConversationId::from_uuid(id);

    let _membership =
        super::verify_membership(&state.db, conversation_id, auth_user.user_id).await?;

    // Resolve the message ID to mark as read.
    let message_id = if let Some(axum::Json(req)) = body {
        req.message_id
    } else {
        None
    };
    let message_id = match message_id {
        Some(mid) => MessageId::from_uuid(mid),
        None => {
            // Fall back to the latest message in the conversation.
            let msg_repo = PgMessageRepo::new(state.db.clone());
            let messages = msg_repo
                .list_paginated(conversation_id, None, 1)
                .await?;
            match messages.first() {
                Some(msg) => msg.id,
                None => return Ok(ApiResponse::new(serde_json::json!({"ok": true}))),
            }
        }
    };

    let cu_repo = PgConversationUserRepo::new(state.db.clone());
    cu_repo
        .mark_read(conversation_id, auth_user.user_id, message_id)
        .await?;

    Ok(ApiResponse::new(serde_json::json!({"ok": true})))
}
```

Add the necessary imports at the top of `conversations.rs`:

```rust
use sober_core::types::MessageId;
use sober_db::repos::PgMessageRepo;
use sober_core::types::MessageRepo;
```

- [ ] **Step 3: Update `chat.subscribe` WS handler mark-read**

In `ws.rs` (around line 358-363), update the mark-read call to query the latest message and pass its ID:

```rust
                // Mark conversation as read for this user (best-effort).
                {
                    let cu_repo = PgConversationUserRepo::new(state.db.clone());
                    let msg_repo = PgMessageRepo::new(state.db.clone());
                    use sober_core::types::{ConversationUserRepo, MessageRepo};
                    if let Ok(messages) = msg_repo.list_paginated(conv_id, None, 1).await {
                        if let Some(latest) = messages.first() {
                            cu_repo.mark_read(conv_id, auth_user.user_id, latest.id).await.ok();
                        }
                    }
                }
```

- [ ] **Step 4: Update conversation list query to include `last_read_message_id`**

In `conversations.rs` repo (around line 180-184), update the SQL query:

```rust
            "SELECT c.id, c.user_id, c.title, c.workspace_id, c.kind, c.agent_mode, c.is_archived, \
             c.created_at, c.updated_at, \
             COALESCE(cu.unread_count, 0) AS unread_count, \
             cu.last_read_message_id, \
             w.name AS workspace_name, w.root_path AS workspace_path \
             FROM conversations c \
             LEFT JOIN workspaces w ON w.id = c.workspace_id \
             LEFT JOIN conversation_users cu ON cu.conversation_id = c.id AND cu.user_id = ",
```

And in the result builder (around line 271), add the field:

```rust
                    unread_count: r.unread_count,
                    last_read_message_id: r.last_read_message_id.map(MessageId::from_uuid),
```

- [ ] **Step 5: Update conversation detail handler to include `last_read_message_id`**

In `routes/conversations.rs` `get_conversation` (around line 171), add the field:

```rust
    let details = ConversationWithDetails {
        conversation,
        unread_count: cu.unread_count,
        last_read_message_id: cu.last_read_message_id,
        tags,
        users,
        workspace_name,
        workspace_path,
    };
```

- [ ] **Step 6: Update conversation create handler**

In `routes/conversations.rs` `create_conversation` (around line 128), add the field to the JSON response that builds `ConversationWithDetails` inline:

```rust
        "unread_count": 0,
        "last_read_message_id": null,
```

- [ ] **Step 7: Build and fix any remaining compile errors**

```bash
cd backend && cargo check -q --workspace 2>&1
```

Fix any callers of the old `mark_read(conversation_id, user_id)` 2-arg signature or `increment_unread(conversation_id, exclude_user_id)` signature that were missed.

- [ ] **Step 8: Run clippy**

```bash
cd backend && cargo clippy -q -- -D warnings 2>&1
```

- [ ] **Step 9: Commit**

```bash
git add backend/crates/sober-api/ backend/crates/sober-db/src/repos/conversations.rs
git commit -m "feat(api): centralize unread in message repo, update mark_read and subscribe"
```

---

### Task 7: Agent — Store User Messages for All Triggers

**Files:**
- Modify: `backend/crates/sober-agent/src/conversation.rs`

- [ ] **Step 1: Remove the human-only guard for user message storage**

In `conversation.rs` (around line 257-277), change the conditional so all triggers store the user message. The message repo's `create` will handle unread increment automatically:

```rust
        // 3. Store user message
        let user_msg = self
            .ctx
            .repos
            .messages()
            .create(CreateMessage {
                conversation_id: self.conversation_id,
                role: MessageRole::User,
                content: content.to_owned(),
                reasoning: None,
                token_count: None,
                metadata: None,
                user_id: Some(user_id),
            })
            .await
            .map_err(|e| AgentError::ContextLoadFailed(e.to_string()))?;
        let user_msg_id = user_msg.id;
```

- [ ] **Step 2: Build and test**

```bash
cd backend && cargo check -q -p sober-agent 2>&1
```

- [ ] **Step 3: Run workspace tests**

```bash
cd backend && cargo test --workspace -q 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-agent/src/conversation.rs
git commit -m "feat(agent): store user messages for all trigger types"
```

---

### Task 8: Agent — Don't Set `user_id` on Assistant Messages

**Files:**
- Modify: `backend/crates/sober-agent/src/turn.rs`

The assistant message in `turn.rs` (around line 437) sets `user_id: Some(params.user_id)`. This means the triggering user is excluded from unread increments for the assistant's response. That's accidentally correct for human triggers (user is viewing) but wrong for scheduler triggers (nobody is viewing, but the owner gets excluded).

The fix: assistant messages should not have a `user_id` — they're authored by the agent, not a user. The unread system correctly handles `user_id: None` by using a nil UUID for exclusion (matches nobody, so everyone gets the unread).

Active users who ARE viewing already call `markRead` on `chat.done`, so their unread resets to 0 immediately.

- [ ] **Step 1: Remove `user_id` from assistant message creation**

In `turn.rs` (around line 426-438), change `user_id: Some(params.user_id)` to `user_id: None`:

```rust
            let assistant_msg = params
                .ctx
                .repos
                .messages()
                .create(CreateMessage {
                    conversation_id: params.conversation_id,
                    role: MessageRole::Assistant,
                    content: text.clone(),
                    reasoning: if reasoning_buffer.is_empty() {
                        None
                    } else {
                        Some(reasoning_buffer.clone())
                    },
                    token_count: usage_stats.map(|u| u.total_tokens as i32),
                    metadata: None,
                    user_id: None,
                })
```

- [ ] **Step 2: Build and test**

```bash
cd backend && cargo check -q -p sober-agent 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add backend/crates/sober-agent/src/turn.rs
git commit -m "fix(agent): don't set user_id on assistant messages — agent is not a user"
```

---

### Task 9: Frontend Types and Services

**Files:**
- Modify: `frontend/src/lib/types/index.ts`
- Modify: `frontend/src/lib/services/conversations.ts`

- [ ] **Step 1: Add `last_read_message_id` to `Conversation` interface**

In `types/index.ts` (around line 39), add to the `Conversation` interface:

```typescript
	last_read_message_id: string | null;
```

- [ ] **Step 2: Update `markRead` service to accept `messageId`**

In `services/conversations.ts` (line 46), update the method:

```typescript
	markRead: (id: string, messageId?: string) =>
		api('/conversations/' + id + '/read', {
			method: 'POST',
			...(messageId ? { body: JSON.stringify({ message_id: messageId }) } : {})
		}),
```

- [ ] **Step 3: Run frontend checks**

```bash
cd frontend && pnpm check 2>&1
```

Expected: type errors in the chat page where `markRead` is called without the new param. That's expected — Task 9 fixes them.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/lib/types/index.ts frontend/src/lib/services/conversations.ts
git commit -m "feat(frontend): add last_read_message_id type and update markRead service"
```

---

### Task 10: Frontend — Unread Divider and Mark-Read Updates

**Files:**
- Modify: `frontend/src/routes/(app)/chat/[id]/+page.svelte`

- [ ] **Step 1: Track `lastReadMessageId` from page data**

Add a derived state near the top of the `<script>` block (near other state declarations):

```typescript
	let lastReadMessageId = $state<string | null>(null);
```

Update the `$effect` that runs on conversation change (around line 165) to set it:

```typescript
		lastReadMessageId = data.conversation.last_read_message_id;
```

- [ ] **Step 2: Add divider rendering in the message loop**

In the `{#each messages as msg}` block (around line 825), add a divider check before each `<ChatMessage>`. Replace the `{#each}` block:

```svelte
			{#each messages as msg, i (msg.id)}
				{#if lastReadMessageId && i > 0 && messages[i - 1].id === lastReadMessageId && msg.id !== lastReadMessageId}
					<div
						class="unread-divider flex items-center gap-3 py-2"
						use:observeUnread
					>
						<div class="h-px flex-1 bg-emerald-500/50"></div>
						<span class="text-xs font-medium text-emerald-600 dark:text-emerald-400">New messages</span>
						<div class="h-px flex-1 bg-emerald-500/50"></div>
					</div>
				{/if}
				<ChatMessage
```

- [ ] **Step 3: Add IntersectionObserver action to reset unread on scroll**

Add an action function in the `<script>` block:

```typescript
	const observeUnread = (node: HTMLElement) => {
		const observer = new IntersectionObserver(
			(entries) => {
				if (entries[0].isIntersecting) {
					lastReadMessageId = null;
					const lastMsg = messages[messages.length - 1];
					if (lastMsg) {
						conversations.markRead(conversationId);
						conversationService.markRead(conversationId, lastMsg.id);
					}
					observer.disconnect();
				}
			},
			{ threshold: 0.5 }
		);
		observer.observe(node);
		return { destroy: () => observer.disconnect() };
	};
```

- [ ] **Step 4: Update `chat.new_message` mark-read calls to pass message ID**

In the `chat.new_message` handler (around line 489-490), update:

```typescript
					untrack(() => conversations.markRead(conversationId));
					conversationService.markRead(conversationId, msg.message_id);
```

- [ ] **Step 5: Update `chat.done` mark-read calls to pass message ID**

In the `chat.done` handler (around line 538-539), update:

```typescript
				untrack(() => conversations.markRead(conversationId));
				conversationService.markRead(conversationId, msg.message_id);
```

- [ ] **Step 6: Update initial mark-read on page load**

In the `$effect` that calls `conversations.markRead` (around line 194-196), also call the API with the latest message ID:

```typescript
		untrack(() => {
			conversations.markRead(data.conversation.id);
			const lastMsg = data.messages[data.messages.length - 1];
			if (lastMsg) {
				conversationService.markRead(data.conversation.id, lastMsg.id);
			}
		});
```

- [ ] **Step 7: Run frontend checks and tests**

```bash
cd frontend && pnpm check 2>&1 && pnpm test --silent 2>&1
```

- [ ] **Step 8: Commit**

```bash
git add frontend/src/routes/(app)/chat/[id]/+page.svelte
git commit -m "feat(frontend): add unread divider with IntersectionObserver reset"
```

---

### Task 11: Backend Tests

**Files:**
- Modify: `backend/crates/sober-api/src/subscribe.rs` (tests module)

- [ ] **Step 1: Update the existing `convert_new_message` test**

The test in `subscribe.rs` tests `conversation_update_to_ws` — it should still pass since that function wasn't changed. Verify:

```bash
cd backend && cargo test -q -p sober-api -- subscribe 2>&1
```

- [ ] **Step 2: Run full workspace tests**

```bash
cd backend && cargo test --workspace -q 2>&1
```

- [ ] **Step 3: Run clippy**

```bash
cd backend && cargo clippy -q -- -D warnings 2>&1
```

- [ ] **Step 4: Run frontend tests**

```bash
cd frontend && pnpm test --silent 2>&1
```

- [ ] **Step 5: Commit any fixes**

If any tests needed updating:

```bash
git add -A && git commit -m "fix: update tests for centralized unread tracking"
```

---

### Task 12: Regenerate sqlx Offline Data and Final Verification

**Files:**
- Modify: `backend/.sqlx/` (generated)

- [ ] **Step 1: Regenerate sqlx prepare data**

```bash
cd backend && cargo sqlx prepare --workspace -q
```

- [ ] **Step 2: Full build from clean**

```bash
cd backend && cargo build -q 2>&1
```

- [ ] **Step 3: Full test suite**

```bash
cd backend && cargo test --workspace -q 2>&1
cd frontend && pnpm check 2>&1 && pnpm test --silent 2>&1
```

- [ ] **Step 4: Commit sqlx data**

```bash
git add backend/.sqlx/
git commit -m "chore: regenerate sqlx offline data for unread tracking changes"
```
