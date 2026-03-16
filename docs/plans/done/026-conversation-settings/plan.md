# 026: Conversation Settings & Message Tags — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a conversation settings slide-over panel and message tag UI to the chat page.

**Architecture:** One new API endpoint (conversation jobs), extend PATCH conversation with workspace_id, new frontend components for settings panel and message tagging. Builds on #025's tag system and ConfirmDialog.

**Tech Stack:** Rust (sqlx, axum), Svelte 5, TypeScript, Tailwind v4

**Design doc:** `docs/plans/active/026-conversation-settings/design.md`

---

## Task 1: Backend — JobRepo filter + conversation jobs endpoint + workspace PATCH

**Files:**
- Modify: `backend/crates/sober-core/src/types/repo.rs` — add `conversation_id: Option<uuid::Uuid>` to `JobRepo::list_filtered`
- Modify: `backend/crates/sober-db/src/repos/jobs.rs` — add conversation_id filter to query
- Modify: `backend/crates/sober-api/src/routes/conversations.rs` — add `GET /{id}/jobs` handler, extend PATCH `UpdateConversationRequest` with `workspace_id`
- Modify: `backend/crates/sober-core/src/types/repo.rs` — add `update_workspace` to `ConversationRepo`
- Modify: `backend/crates/sober-db/src/repos/conversations.rs` — implement `update_workspace`

- [ ] Move plan to active/: `git mv docs/plans/pending/026-conversation-settings docs/plans/active/026-conversation-settings`
- [ ] Add `conversation_id: Option<uuid::Uuid>` param to `JobRepo::list_filtered` in repo.rs
- [ ] Update `PgJobRepo::list_filtered` in jobs.rs to filter by conversation_id when set
- [ ] Update ALL existing callers of `list_filtered` to pass `None` for new param (check sober-api, sober-scheduler, sober-cli)
- [ ] Add `update_workspace(id: ConversationId, workspace_id: Option<WorkspaceId>)` to `ConversationRepo` trait
- [ ] Implement in `PgConversationRepo`: `UPDATE conversations SET workspace_id = $2 WHERE id = $1`
- [ ] Add `list_conversation_jobs` handler in conversations.rs: verify ownership, call `list_filtered` with conversation_id
- [ ] Register route: `.route("/conversations/{id}/jobs", get(list_conversation_jobs))`
- [ ] Extend `UpdateConversationRequest` with `workspace_id: Option<Option<String>>` using `#[serde(default, deserialize_with = "...")]` or skip_serializing_if pattern. When present, call `update_workspace`
- [ ] Build: `cd backend && cargo build -q && cargo clippy -q -- -D warnings && cargo test --workspace -q`
- [ ] Commit: `feat(api): add conversation jobs endpoint, extend PATCH with workspace_id`

---

## Task 2: Frontend — Types and Services

**Files:**
- Modify: `frontend/src/lib/types/index.ts`
- Create: `frontend/src/lib/services/jobs.ts`
- Modify: `frontend/src/lib/services/conversations.ts`

- [ ] Add `Job` interface to types/index.ts
- [ ] Create `frontend/src/lib/services/jobs.ts` with `jobService.listByConversation()`
- [ ] Add `updateWorkspace(id, workspaceId)` to conversationService
- [ ] `cd frontend && pnpm check`
- [ ] Commit: `feat(frontend): add Job type and job/workspace services`

---

## Task 3: Frontend — Settings Panel Components

**Files:**
- Create: `frontend/src/lib/components/SettingsSection.svelte`
- Create: `frontend/src/lib/components/JobList.svelte`
- Create: `frontend/src/lib/components/ConversationSettings.svelte`

- [ ] Create `SettingsSection.svelte` — section wrapper with title, optional description, children snippet, optional `danger` flag for red border
- [ ] Create `JobList.svelte` — read-only job list with status badges (active=emerald, paused=amber, cancelled=zinc, running=sky), schedule, next run time. Empty state message
- [ ] Create `ConversationSettings.svelte` — slide-over panel (~400px, full-width mobile). Animated slide+fade. Closes on Escape/click-outside/close button. Sections: info, title (save on blur/Enter), permission mode (3-button), workspace dropdown, tags (TagInput), scheduled jobs (JobList), danger zone (archive/clear/delete with ConfirmDialog). Loads jobs and workspaces on open
- [ ] `cd frontend && pnpm check`
- [ ] Commit: `feat(frontend): add conversation settings panel with job list`

---

## Task 4: Frontend — Integrate Settings Panel in Chat Page

**Files:**
- Modify: `frontend/src/routes/(app)/chat/[id]/+page.svelte`

- [ ] Add `settingsOpen` state and gear icon button in header
- [ ] Render ConversationSettings, wire callbacks to existing handlers
- [ ] Sync permissionMode between panel and status bar (same state variable)
- [ ] Move archive/delete buttons from header to settings panel danger zone (remove from header)
- [ ] `cd frontend && pnpm check && pnpm lint`
- [ ] Commit: `feat(frontend): integrate settings panel in chat page`

---

## Task 5: Frontend — Message Action Bar and Tag Popover

**Files:**
- Create: `frontend/src/lib/components/MessageActionBar.svelte`
- Create: `frontend/src/lib/components/MessageTagPopover.svelte`

- [ ] Create `MessageActionBar.svelte` — floating toolbar on hover with tag icon and delete icon buttons
- [ ] Create `MessageTagPopover.svelte` — popover with autocomplete tag input, shows applied tags with "x" to remove. Calls tagService.addToMessage/removeFromMessage. Closes on Escape/click-outside
- [ ] `cd frontend && pnpm check`
- [ ] Commit: `feat(frontend): add message action bar and tag popover components`

---

## Task 6: Frontend — Integrate Message Tags in ChatMessage

**Files:**
- Modify: `frontend/src/lib/components/ChatMessage.svelte`
- Modify: `frontend/src/routes/(app)/chat/[id]/+page.svelte`

- [ ] Add `tags`, `messageId`, `onTagsChange` props to ChatMessage
- [ ] Show tag pills below message bubble (left-aligned for assistant, right-aligned for user). 3+ tags: show first 2 + "+N more" expandable
- [ ] Integrate MessageActionBar on hover (tag icon opens MessageTagPopover, delete icon uses existing onDelete)
- [ ] In chat page, add `messageTags` state map (`Record<string, Tag[]>`). Pass to ChatMessage. Update on tag change
- [ ] `cd frontend && pnpm check && pnpm lint`
- [ ] Commit: `feat(frontend): add message tag display and action bar to chat messages`

---

## Task 7: Integration, Version Bump, Cleanup

- [ ] `cd backend && cargo build -q && cargo test --workspace -q && cargo clippy -q -- -D warnings`
- [ ] `cd frontend && pnpm check && pnpm lint`
- [ ] Move plan to done/: `git mv docs/plans/active/026-conversation-settings docs/plans/done/026-conversation-settings`
- [ ] Version bump (MINOR) on affected crates: sober-core, sober-db, sober-api, frontend/package.json
- [ ] Commit: `feat(026): conversation settings and message tags — integration and version bump`
