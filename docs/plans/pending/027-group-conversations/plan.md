# Plan 027: Group Conversations

## Goal

Enable multi-user group conversations with member management, configurable
agent behavior, and timeline events. Depends on #025 and #026 being
implemented first.

## Changes

### Backend

1. **Database migration**
   - `backend/migrations/YYYYMMDD_group_conversations.sql`
   - `ALTER TYPE user_role ADD VALUE 'admin'`
   - `ALTER TYPE message_role ADD VALUE 'event'`
   - `CREATE TYPE agent_mode AS ENUM ('always', 'mention', 'silent')`
   - `conversations`: add `agent_mode agent_mode NOT NULL DEFAULT 'always'`
   - `messages`: add `metadata JSONB`

2. **Backend types (sober-core)**
   - `types/enums.rs` — add `AgentMode` (Always/Mention/Silent), add `Admin`
     to `UserRole`, add `Event` to `MessageRole`
   - `types/domain.rs` — add `agent_mode: AgentMode` to `Conversation`, add
     `metadata: Option<serde_json::Value>` to `Message`
   - `types/repo.rs` — extend `ConversationUserRepo` with `list_members`,
     `update_role`, `delete`. Extend `ConversationRepo` with
     `update_agent_mode`. Extend `UserRepo` with `find_by_username`

3. **Pg implementations (sober-db)**
   - `repos/conversation_users.rs` — implement `list_members` (JOIN users for
     username), `update_role`, `delete`
   - `repos/conversations.rs` — implement `update_agent_mode`
   - `repos/users.rs` — implement `find_by_username`
   - `rows.rs` — add `ConversationUserWithUsernameRow`

4. **Authorization refactor (sober-api)**
   - Update all conversation-scoped endpoints and WebSocket handlers
     (`chat.subscribe`, `chat.message`) to check `conversation_users`
     membership instead of `conversations.user_id`
   - Non-members get 404

5. **Member management endpoints (sober-api)**
   - `routes/conversations.rs` — new handlers:
     - `GET /conversations/{id}/members` — list with roles
     - `POST /conversations/{id}/members` — add by username (idempotent)
     - `PATCH /conversations/{id}/members/{user_id}` — change role
     - `DELETE /conversations/{id}/members/{user_id}` — kick
     - `POST /conversations/{id}/leave` — self-leave
   - Each mutation inserts an event message (role=event, content=display
     string, metadata=structured JSON)
   - Extend `POST /conversations` to accept `kind: 'group'`, required title,
     optional `members` array
   - Add `agent_mode` to `PATCH /conversations/{id}`

6. **WebSocket changes (sober-api)**
   - `routes/ws.rs` — add `ChatMemberAdded`, `ChatMemberRemoved`,
     `ChatRoleChanged` variants to `ServerWsMessage`
   - Member change events sent via `UserConnectionRegistry` to all
     conversation members
   - Kicked users receive `chat.member_removed` with their own user_id →
     frontend unsubscribes and navigates away

7. **Agent mode handling (sober-agent)**
   - `handle_message` flow — after storing message, look up conversation's
     `agent_mode`. If `mention` and no `@sober` in content, or if `silent`,
     return message_id without running LLM pipeline
   - `sober-mind` prompt assembly — exclude `event` role messages, prefix
     user messages with `[username]:` in group conversations

### Frontend

8. **Types and services**
   - `types/index.ts` — add `AgentMode`, update `MessageRole` with `Event`,
     add `metadata` to `Message`, add member-related WS message types
   - `services/conversations.ts` — add `listMembers`, `addMember`,
     `updateMemberRole`, `removeMember`, `leave`, `updateAgentMode`

9. **Group creation UI**
   - New `CreateGroupDialog.svelte` — title input (required) + member
     autocomplete
   - Modify `ConversationList.svelte` — "New conversation" becomes dropdown
     with "New direct" / "New group", group icon for group conversations

10. **Member management in settings panel**
    - New `MemberList.svelte` — members with role badges, role change dropdown,
      remove button (owner/admin), leave button (members)
    - New `AddMemberInput.svelte` — username autocomplete
    - Modify `ConversationSettings.svelte` — add members section and agent
      mode selector (visible for groups only)

11. **Message display changes**
    - Modify `ChatMessage.svelte` — show sender username above bubble in
      group conversations, render event messages as centered muted text
    - Handle `chat.member_removed` with own user_id → unsubscribe, navigate
      to dashboard, show notice

## Acceptance Criteria

- Group conversations can be created with title and optional initial members
- Members can be added by username (owner/admin), idempotent for existing
- Roles changeable: owner can promote to admin or demote
- Members can be kicked (owner/admin) and leave (self)
- Kicked users' WebSocket disconnects from conversation gracefully
- Timeline shows event messages for all member changes
- Agent mode configurable: always/mention/silent
- Agent responds to `@sober` mentions in mention mode, silent in silent mode
- Agent ignores event messages in prompt assembly
- Group messages show sender usernames
- All conversation-scoped auth checks use `conversation_users` membership
- `cargo build -q`, `cargo clippy -q -- -D warnings`, `cargo test --workspace -q`
- `pnpm check` and `pnpm build --silent` pass
