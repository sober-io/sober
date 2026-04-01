# #046 Centralized Unread Tracking — Design

## Problem

Unread message tracking is broken for scheduler-triggered messages and inconsistent across message sources. The current implementation has three gaps:

1. **User messages in groups** — `ws.rs` broadcasts to group members but never increments unread counts
2. **Scheduler user messages** — `conversation.rs` skips storing user messages for non-human triggers
3. **Assistant messages from scheduler** — the exclusion logic in `subscribe.rs` excludes the automation owner, so in a 1:1 conversation nobody gets unread

Root cause: unread increment is done in `subscribe.rs` event handler — a single consumer of `NewMessage` events from the agent broadcast channel. Only assistant messages produce `NewMessage` events, and the exclusion logic doesn't account for trigger source.

## Solution

Centralize unread tracking at the message storage layer. Every message stored via `MessageRepo::create` automatically increments unread counts for conversation members who haven't read up to that point.

### Key Changes

**`last_read_message_id`** — New column on `conversation_users` tracking the exact read position (UUIDv7 message ID). Replaces timestamp-based `last_read_at` as the primary read cursor. Enables "unread from here" dividers in the frontend.

**`MessageRepo::create` side-effect** — After inserting a message, runs `UPDATE conversation_users SET unread_count = unread_count + 1 WHERE conversation_id = $1 AND user_id != $3 AND (last_read_message_id IS NULL OR last_read_message_id < $2)`. The author (`$3`) is excluded so users don't get unread for their own messages. Assistant messages have `user_id = None`, so all members get incremented.

**`mark_read(conversation_id, user_id, message_id)`** — Updated signature accepts the last-read message ID. Sets `last_read_message_id` and resets `unread_count = 0`.

**Subscribe cleanup** — `handle_new_message_unread` in `subscribe.rs` is removed. Replaced with `push_unread_notifications` that reads current counts from DB and pushes `chat.unread` WS events for live updates.

**Agent changes** — Store user messages for all trigger types (not just human). Remove `user_id` from assistant message creation — the agent is not a user.

**Frontend divider** — Chat page renders an "unread from here" divider after the `last_read_message_id` message. IntersectionObserver resets the divider when it scrolls into view.

### Unread Lifecycle

1. Message stored → `MessageRepo::create` increments unread for non-author members whose `last_read_message_id < new_message_id`
2. `NewMessage` event → `subscribe.rs` pushes `chat.unread` WS notification to connected users
3. User opens conversation → `chat.subscribe` calls `mark_read` with latest message ID → resets to 0
4. User is actively viewing → `chat.new_message` / `chat.done` handlers call `markRead` immediately
5. User returns after being offline → API returns `unread_count` from DB in conversation list

### What Gets Removed

- `handle_new_message_unread` function in `subscribe.rs`
- `exclude_user_id` parameter on `increment_unread` (replaced by `author_user_id`)
- Human-only guard for user message storage in `conversation.rs`
- `user_id` on assistant message creation in `turn.rs`
