# Plan 026: Conversation Settings & Message Tags

## Goal

Add a conversation settings slide-over panel, read-only scheduler job listing
per conversation, and message tag UI. Depends on #025 being implemented first.

## Changes

### Backend

1. **`JobRepo::list_filtered` — add conversation_id param**
   - `sober-core/src/types/repo.rs` — add `conversation_id: Option<uuid::Uuid>`
     to `list_filtered` signature
   - `sober-db/src/repos/jobs.rs` — add `AND conversation_id = $N` clause when
     param is set

2. **`ConversationRepo::update_workspace` — new method**
   - `sober-core/src/types/repo.rs` — add `update_workspace(id, workspace_id)` to
     `ConversationRepo` trait
   - `sober-db/src/repos/conversations.rs` — implement with
     `UPDATE conversations SET workspace_id = $1 WHERE id = $2`

3. **`PATCH /conversations/{id}` — add workspace_id**
   - `sober-api/src/routes/conversations.rs` — add `workspace_id` to
     `UpdateConversationRequest` with `Option<Option<WorkspaceId>>` semantics
     (absent = don't change, null = unlink, value = set). Use serde
     `deserialize_with` or wrapper type. Call `repo.update_workspace()` when
     field is present

4. **`GET /conversations/{id}/jobs` — new handler**
   - `sober-api/src/routes/conversations.rs` — new `list_conversation_jobs`
     handler. Verify conversation membership via `conversation_users`, then call
     `job_repo.list_filtered()` with `conversation_id` set. Return job array
   - Register route: `/conversations/{id}/jobs` (GET)

### Frontend

5. **Types and services**
   - `frontend/src/lib/types/index.ts` — add `Job` interface
   - `frontend/src/lib/services/jobs.ts` — new file with
     `jobService.listByConversation()`
   - `frontend/src/lib/services/conversations.ts` — add `updateWorkspace(id, workspaceId)`

6. **Message action bar and tag UI**
   - New `frontend/src/lib/components/MessageActionBar.svelte` — floating
     toolbar on hover with tag and delete icons
   - New `frontend/src/lib/components/MessageTagPopover.svelte` — autocomplete
     tag input, shows applied tags with remove
   - Modify `frontend/src/lib/components/ChatMessage.svelte` — wrap message
     in hover container, integrate action bar, render tag pills below bubble
   - Tag pills: colored, left-aligned for assistant, right-aligned for user,
     overflow "+N more" at 3+

7. **Conversation settings panel**
   - New `frontend/src/lib/components/ConversationSettings.svelte` — slide-over
     panel (~400px), absolute positioned in chat page, animated slide + fade.
     Sections: info, title (save on blur/Enter), permission mode (syncs with
     status bar), workspace (dropdown), tags (reuse TagInput), scheduled jobs
     (JobList), danger zone (archive/clear/delete with ConfirmDialog)
   - New `frontend/src/lib/components/SettingsSection.svelte` — minimal
     section wrapper (<20 lines): heading, optional description, content
     snippet, consistent spacing/borders
   - New `frontend/src/lib/components/JobList.svelte` — read-only list of jobs
     with name, schedule, status badge, next run. Empty state message
   - Modify `frontend/src/routes/(app)/chat/[id]/+page.svelte` — add settings
     icon in header, toggle `ConversationSettings` panel, pass conversation
     data and callbacks

## Acceptance Criteria

- Settings panel opens/closes with slide animation from right
- Panel closes on Escape, click outside, or settings button
- Title editable, saves on blur/Enter
- Permission mode syncs bidirectionally with status bar
- Workspace changeable via dropdown, unlinkable
- Tags manageable in settings panel (reuses sub-spec 1 TagInput)
- Scheduled jobs listed read-only with status badges
- Danger zone: archive, clear history, delete (with confirmations, inbox protected)
- Message hover shows action bar with tag and delete icons
- Message tag popover with autocomplete, add/remove tags
- Tag pills display below messages, overflow at 3+
- `cargo build -p sober-api -q`, `cargo clippy -p sober-api -q -- -D warnings`,
  `cargo test -p sober-api -q` pass
- `pnpm check` and `pnpm build --silent` pass
