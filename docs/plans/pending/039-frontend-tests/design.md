# Frontend Test Coverage Design

Add basic test coverage across the entire frontend: pure utilities, reactive stores, and Svelte 5 components.

## Infrastructure

### Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `@testing-library/svelte` | ^5 | Svelte 5 component rendering + DOM queries |
| `@testing-library/jest-dom` | ^6 | DOM assertion matchers (`toBeInTheDocument`, etc.) |
| `@testing-library/user-event` | ^14 | Realistic user interaction simulation |
| `jsdom` | ^26 | DOM environment for vitest |

### Config Changes

**`vite.config.ts`** -- switch import from `'vite'` to `'vitest/config'`, add `svelteTesting()` plugin after `sveltekit()`, add `test` block:

```ts
import { svelteTesting } from '@testing-library/svelte/vite';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [tailwindcss(), sveltekit(), svelteTesting()],
  test: {
    environment: 'jsdom',
    setupFiles: ['./vitest-setup.ts'],
    include: [
      'src/**/*.{test,spec}.{js,ts}',
      'src/**/*.svelte.{test,spec}.{js,ts}'
    ]
  }
});
```

The `svelteTesting()` plugin fixes the SSR transform issue (ensures `resolve.conditions` includes `'browser'`) and adds automatic cleanup between tests. **Plugin order matters**: `svelteTesting()` must come after `sveltekit()` because it patches the Svelte plugin's resolve configuration. The websocket store's `svelte/reactivity` imports (`SvelteMap`, `SvelteSet`) also depend on the `browser` condition being set correctly.

**`vitest-setup.ts`** -- global setup file:

```ts
import '@testing-library/jest-dom/vitest';
```

**`tsconfig.json`** -- add `@testing-library/jest-dom` to `compilerOptions.types` so custom matchers (`toBeInTheDocument()`, etc.) are recognized by TypeScript. Alternatively, ensure `vitest-setup.ts` is within the TypeScript `include` scope.

### File Naming Convention

| Test type | Extension | Reason |
|-----------|-----------|--------|
| Pure TS (utilities, services) | `*.test.ts` | No Svelte compiler needed |
| Rune-using (stores with `$state`/`$derived`) | `*.svelte.test.ts` | Svelte compiler transforms runes |
| Component rendering | `*.test.ts` | `@testing-library/svelte` handles rendering; no runes in test file |

### Mocking Strategy

- **`fetch`** -- global mock via `vi.fn()` per test file
- **Services** -- `vi.mock('$lib/services/...')` for component tests that call APIs
- **WebSocket** -- mock constructor + instance methods on `globalThis.WebSocket`
- **`location`** -- mock `globalThis.location` for WebSocket store protocol detection
- **Timers** -- `vi.useFakeTimers()` for reconnect delay testing
- No mock server -- unnecessary for this scope

### Testing `$effect`

`$effect` works correctly in `.svelte.test.ts` files with Svelte 5.53.7 + Vitest 3.2.4. The previously reported issues ([svelte#16092](https://github.com/sveltejs/svelte/issues/16092), [svelte#15051](https://github.com/sveltejs/svelte/issues/15051)) are both closed — fixed in Vitest 3.2.3.

Pattern for effect tests:
- Wrap in `$effect.root()` and call the returned cleanup function
- Call `flushSync()` from `'svelte'` before assertions
- Use plain arrays (not `$state`) for accumulator variables inside effects

---

## Tier 1: Utility Tests

### `src/lib/utils/api.test.ts` (6 tests)

Tests the `api()` fetch wrapper and `ApiError` class.

| Test | What it verifies |
|------|-----------------|
| Unwraps `{ data: T }` envelope | Successful responses return inner `data` value |
| Prepends `/api/v1`, sets defaults | Path prefixing, `Content-Type: application/json`, `credentials: 'include'` |
| Spreads custom RequestInit | Caller-provided method, body, headers are forwarded to fetch |
| Throws ApiError on non-ok | Status, code, and message extracted from error response body |
| Fallback on malformed error body | Code defaults to `"unknown"`, message to `"API error {status}"` |
| Handles non-JSON error body | `.json().catch(() => ({}))` path -- body parse failure still produces ApiError |

### `src/lib/utils/markdown.test.ts` (6 tests)

Tests `renderMarkdown()` -- Marked + DOMPurify pipeline.

| Test | What it verifies |
|------|-----------------|
| Basic markdown | Bold, italic, code blocks, headings, lists render correctly |
| GFM features | Tables, strikethrough, line breaks (`breaks: true`) |
| Strips `<script>` | XSS: script tags removed from output |
| Strips FORBID_TAGS | `<iframe>`, `<object>`, `<embed>`, `<form>`, `<style>` all removed |
| Passes allowed tags | Tables, images with `src`/`alt` attributes preserved |
| Blocks data attributes | `ALLOW_DATA_ATTR: false` strips `data-*` attributes |

### `src/lib/utils/time.test.ts` (2 tests)

| Test | What it verifies |
|------|-----------------|
| `formatRelativeTime` | Returns human-readable past time (e.g., "2 hours ago") |
| `formatRelativeFuture` | Returns human-readable future time (e.g., "in 3 days") |

Note: both functions share the same `dayjs().fromNow()` implementation. Tests verify dayjs handles both directions correctly. The redundant function is a pre-existing issue outside this plan's scope.

---

## Tier 2: Store Tests

### `src/lib/stores/auth.svelte.test.ts` (4 tests)

Tests the `auth` reactive store. File uses `.svelte.test.ts` extension for rune support.

| Test | What it verifies |
|------|-----------------|
| Initial state | `user` is null, `loading` is true, `isAuthenticated` is false |
| `setUser` with value | Updates `user`, `isAuthenticated` becomes true |
| `setUser(null)` | Resets `isAuthenticated` to false |
| `setLoading` | Updates `loading` state |

### `src/lib/stores/conversations.svelte.test.ts` (10 tests)

Tests the `conversations` reactive store. File uses `.svelte.test.ts` extension for rune support.

| Test | What it verifies |
|------|-----------------|
| `set` | Replaces entire items list |
| `prepend` | Adds conversation to front of list |
| `updateTitle` | Updates title on matching conversation by id |
| `remove` | Filters out conversation by id |
| `updateUnread` | Sets unread count, re-sorts: unread first, then by `updated_at` desc |
| `updateUnread` sort stability | Two unread items remain sorted by `updated_at` |
| `markRead` | Sets `unread_count` to 0 on matching conversation |
| `archive` / `unarchive` | Toggles `is_archived` flag |
| `updateTags` | Replaces tags array on matching conversation |
| `update` | Merges partial fields into matching conversation |

### `src/lib/stores/websocket.svelte.test.ts` (6 tests)

Tests the `websocket` singleton. Requires mock `WebSocket` class and `location` on globalThis.

| Test | What it verifies |
|------|-----------------|
| Initial state | `connected` is false, `error` is null |
| Send queues when disconnected | Messages buffered in `pendingQueue`, flushed on `onopen` |
| `subscribe` registers handler | Handler stored, `chat.subscribe` message sent |
| Unsubscribe removes handler | Returned function cleans up handler and subscription set |
| `disconnect` clears state | Handlers, subscriptions, queue all reset; `connected` false |
| Reconnect scheduling | After `onclose`, reconnects with exponential backoff delays via fake timers |

---

## Tier 3: Component Tests

All component tests use `render` + `screen` from `@testing-library/svelte` and `userEvent` from `@testing-library/user-event`. Services are mocked at module level.

### `src/lib/components/ChatMessage.test.ts` (10 tests)

| Test | What it verifies |
|------|-----------------|
| User message alignment | Right-aligned with dark background classes |
| Assistant message alignment | Left-aligned with light background classes |
| Event role rendering | Centered italic text, no message bubble |
| Content rendering | Markdown processed and rendered as HTML |
| Thinking indicator | Shows `ThinkingIndicator` when `thinking=true`, `content` empty |
| Streaming mode | Shows `StreamingText` component when `streaming=true` |
| Tool calls display | Renders `ToolCallDisplay` for each tool call |
| Tag display | Renders first 2 tags, shows "+N more" button when 3+ tags present |
| Expand hidden tags | Clicking "+N more" reveals all tags |
| Action bar visibility | Hidden during streaming, ephemeral, or thinking states |

### `src/lib/components/ChatInput.test.ts` (7 tests)

| Test | What it verifies |
|------|-----------------|
| Send button disabled | Disabled when input is empty/whitespace |
| Enter submits | Calls `onsend` with trimmed value, clears input |
| Shift+Enter no submit | Does not trigger send (allows multiline) |
| Slash palette shown | Typing "/" shows `SlashCommandPalette`; typing "/" then space hides it |
| Builtin commands | `/help`, `/clear` etc. trigger `onSlashCommand` callback |
| Skill slash commands | Non-builtin `/foo` goes through `onsend` |
| Busy state | Button text shows "Queue" when `busy=true` |

### `src/lib/components/ConversationSettings.test.ts` (8 tests)

Mocks: `conversationService`, `jobService`. The component's `$effect` fires `loadJobs` and `loadCollaborators` on open -- mocked services return empty arrays by default, with specific tests overriding as needed via `vi.mocked(...).mockResolvedValue(...)`.

| Test | What it verifies |
|------|-----------------|
| Closed state | Renders nothing when `open=false` |
| Open state | Renders panel with title, kind label, created date |
| Title editing | Blur on input calls `onUpdateTitle` with trimmed value |
| Archive button text | Toggles "Archive" / "Unarchive" based on `is_archived` |
| Clear history confirm | Button opens confirm dialog; confirming calls `onClearHistory` |
| Delete hidden for inbox | "Delete conversation" button not rendered for inbox kind |
| Delete confirm | Confirm dialog calls `onDelete` |
| Close interactions | Close button and backdrop click call `onClose` |

### `src/lib/components/TagInput.test.ts` (6 tests)

Mocks: `tagService`.

| Test | What it verifies |
|------|-----------------|
| Renders existing tags | Each tag displayed with name and remove button |
| Add button reveals input | Clicking "Add tag" shows input field |
| Enter submits tag | Calls `onAdd` with trimmed input value |
| Escape closes input | Hides input, clears value |
| Remove button | Calls `onRemove` with correct tag id |
| Suggestion filtering | Suggestions exclude already-applied tags |

### `src/lib/components/MessageTagPopover.test.ts` (5 tests)

Mocks: `tagService`.

| Test | What it verifies |
|------|-----------------|
| Renders existing tags | Tags displayed with remove buttons |
| Input filtering | Suggestion list filtered by name, case-insensitive |
| Excludes applied tags | Already-applied tags hidden from suggestions |
| Add via suggestion | Clicking suggestion calls `tagService.addToMessage`, fires `onTagsChange` |
| Escape closes | Escape key calls `onClose` |

---

## Summary

| Tier | Files | Tests | Scope |
|------|-------|-------|-------|
| 1 - Utilities | 3 | 14 | `api.ts`, `markdown.ts`, `time.ts` |
| 2 - Stores | 3 | 20 | `auth`, `conversations`, `websocket` |
| 3 - Components | 5 | 36 | `ChatMessage`, `ChatInput`, `ConversationSettings`, `TagInput`, `MessageTagPopover` |
| **Total** | **11** | **70** | |
