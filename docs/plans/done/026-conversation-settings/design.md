# Design 026: Conversation Settings & Message Tags

> Sub-spec 2 of 3. Covers: conversation settings panel, read-only scheduler
> job listing, message tag UI.
>
> Depends on: sub-spec 1 (#025 — conversation improvements core).
>
> Sub-spec 3: Group conversations, invitations, shared context.

## Context

Sub-spec 1 added conversation tags, archiving, hard delete, and the
`message_tags` schema + API. This sub-spec builds the UI for managing
conversation settings in one place and surfaces message tagging in the
frontend. It also adds a read-only view of scheduler jobs linked to a
conversation.

## Decisions

- **Settings panel** is a slide-over from the right, not a modal or separate
  route. Keeps the user in chat context.
- **Scheduler jobs** are read-only within the conversation. Full scheduler
  management is a separate future feature.
- **Move messages** between conversations — deferred entirely.
- **Model hint / system prompt overrides** — deferred (no backend support yet).
- **Permission mode** remains in the chat status bar AND appears in settings
  panel. Both stay in sync.
- **Message tag filtering/search** — deferred. Tags are visual markers only
  for now.

---

## 1. Backend API

### 1.1 New endpoint

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/v1/conversations/{id}/jobs` | List jobs linked to this conversation |

**Response:** Array of jobs where `jobs.conversation_id = {id}`. Filtered to
jobs the authenticated user can see:
- `owner_type = 'user'` AND `owner_id = auth_user.user_id`
- OR `owner_type = 'system'` (viewable by all)

**Response fields per job:** `id`, `name`, `schedule`, `status`, `next_run_at`,
`last_run_at`.

### 1.2 Repository changes

**File:** `backend/crates/sober-core/src/types/repo.rs`

Extend existing `list_filtered` in `JobRepo` trait with an optional
`conversation_id` parameter rather than adding a new method. This keeps the
repo surface area small.

**File:** `backend/crates/sober-db/src/repos/jobs.rs`

Add `conversation_id: Option<uuid::Uuid>` to `list_filtered`. When set, adds
`AND conversation_id = $N` to the query. The handler calls `list_filtered` with
`conversation_id` set and `owner_id`/`owner_type` filters for authorization.

### 1.3 Extend existing PATCH endpoint

The existing `PATCH /conversations/{id}` from sub-spec 1 accepts `title`,
`permission_mode`, and `archived`. This sub-spec adds `workspace_id` to the
`UpdateConversationRequest`:
- Uses `Option<Option<WorkspaceId>>` semantics: field absent = don't change,
  `"workspace_id": null` = unlink, `"workspace_id": "uuid"` = set. Implemented
  via `#[serde(default, deserialize_with = "...")]` or a wrapper type
- Calls `repo.update_workspace(id, workspace_id)` (new repo method, accepts
  `Option<WorkspaceId>` — `None` unlinks, `Some` sets)

### 1.4 Authorization

The `GET /conversations/{id}/jobs` endpoint must first verify the authenticated
user owns/is a member of the conversation before querying jobs. Uses
`conversation_users` from sub-spec 1 (which must be implemented first). Consistent
with other conversation-scoped endpoints.

### 1.5 Other existing APIs

All other APIs needed by the settings panel already exist from sub-spec 1 and
the current codebase:
- `PATCH /conversations/{id}` — title, permission_mode, archived (+ workspace_id above)
- `GET /workspaces` — existing endpoint to populate workspace dropdown
- `POST/DELETE /conversations/{id}/tags` — tag management
- `DELETE /conversations/{id}/messages` — clear history
- `DELETE /conversations/{id}` — hard delete
- `POST/DELETE /messages/{id}/tags` — message tagging

---

## 2. Frontend — Conversation Settings Panel

### 2.1 Trigger

Settings icon button in the chat page header (gear icon or similar). Clicking
toggles the panel open/closed.

### 2.2 Panel behavior

- Slides in from the right (~400px wide on desktop, full-width on mobile)
- Animated slide + fade transition
- Closes on: Escape key, clicking outside, clicking the settings button again
- Changes save immediately (no "save" button)

### 2.3 Sections (top to bottom)

**Info** — read-only metadata:
- Conversation kind (direct/inbox)
- Created date
- Message count

**Title** — editable inline text field. Saves on blur or Enter (not on every
keystroke). Calls `PATCH /conversations/{id}` with `{ "title": "..." }`.

**Permission mode** — three-button selector matching the status bar
(interactive / policy-based / autonomous). Changes sync bidirectionally with
the status bar.

**Workspace** — shows linked workspace name if any. Dropdown to change or
unlink. Calls `PATCH /conversations/{id}` with `{ "workspace_id": "..." }` or
`{ "workspace_id": null }`.

**Tags** — tag management with autocomplete input. Same component as the chat
header tag area from sub-spec 1. Add/remove tags.

**Scheduled jobs** — read-only list from `GET /conversations/{id}/jobs`:
- Each job shows: name, schedule (human-readable), status badge
  (active/paused/cancelled/running), next run time
- Empty state: "No scheduled jobs for this conversation"

**Danger zone** — red-bordered section:
- Archive / Unarchive button
- Clear history button (with confirmation dialog)
- Delete conversation button (hidden for inbox, with confirmation dialog)

### 2.4 Components

- **New:** `ConversationSettings.svelte` — the slide-over panel, rendered inside
  the chat page component (`+page.svelte`). Uses absolute positioning over the
  main content area. Click-outside detection relative to the panel element
- **New:** `SettingsSection.svelte` — reusable section wrapper (title + content)
- **New:** `JobList.svelte` — read-only scheduler job list
- **Reuse:** `TagInput.svelte` from sub-spec 1
- **Reuse:** `ConfirmDialog.svelte` from sub-spec 1

---

## 3. Frontend — Message Tag UI

### 3.1 Message action bar

Hover over any message reveals a floating action bar (small toolbar) near the
message bubble. Actions:
- **Tag icon** — opens tag popover
- **Delete icon** — triggers delete flow (from sub-spec 1)

The action bar appears on hover (desktop only). Mobile interaction (long-press)
is deferred.

### 3.2 Tag popover

Clicking the tag icon opens a small popover anchored to the action bar:
- Text input with autocomplete filtering existing tags
- Enter to apply tag (auto-creates if new name)
- Shows currently applied tags with "x" to remove
- Closes on click outside or Escape

### 3.3 Tag display on messages

- Small colored pills below the message bubble
- Left-aligned for assistant messages, right-aligned for user messages
- If 3+ tags, show first 2 + "+N more" that expands on click

### 3.4 Components

- **New:** `MessageActionBar.svelte` — floating toolbar on hover
- **New:** `MessageTagPopover.svelte` — tag management popover
- **Modify:** `ChatMessage.svelte` — integrate action bar and tag pill display

---

## 4. Frontend Services

### 4.1 New service

**File:** `frontend/src/lib/services/jobs.ts`

```typescript
export const jobService = {
  listByConversation: (conversationId: string) =>
    api<Job[]>(`/conversations/${conversationId}/jobs`),
};
```

### 4.2 New types

**File:** `frontend/src/lib/types/index.ts`

```typescript
interface Job {
  id: string;
  name: string;
  schedule: string;
  status: 'active' | 'paused' | 'cancelled' | 'running';
  next_run_at: string;
  last_run_at: string | null;
}
```
