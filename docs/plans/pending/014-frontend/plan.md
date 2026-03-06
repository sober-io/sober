# 014 — Frontend: Implementation Plan

Date: 2026-03-06

---

## Steps

1. **Verify SvelteKit scaffold from phase 002.** Confirm the project exists under
   `frontend/` with Tailwind configured via `@tailwindcss/vite`. If not, bootstrap it.

2. **Create type definitions** in `$lib/types/index.ts`. Define `User`, `Conversation`,
   `Message`, `ToolCall`, `McpServer`, and `WsMessage` types that mirror backend responses.

3. **Create API client** in `$lib/utils/api.ts`. Thin typed wrapper around `fetch` with
   cookie credentials, JSON content type, and `ApiError` class.

4. **Create auth store** in `$lib/stores/auth.svelte.ts`. Reactive state using `$state`
   rune, exposing `user`, `loading`, `isAuthenticated` via getters.

5. **Create WebSocket store** in `$lib/stores/websocket.svelte.ts`. Factory function
   returning reactive connection state with `connect`, `disconnect`, `send`, and
   `onMessage` methods.

6. **Create root `+layout.ts`** that calls `GET /api/v1/auth/me` and populates the auth
   store. Handle 401 gracefully by setting user to null (do not redirect).

7. **Create `(public)` layout group** with minimal layout. Build login page
   (`(public)/login/+page.svelte`) and register page (`(public)/register/+page.svelte`).
   Register page shows "pending approval" message after successful submission.

8. **Create `(app)` layout group** with sidebar layout containing `ConversationList`.
   Add `+layout.ts` that checks auth state and redirects to `/login` if unauthenticated.

9. **Create conversation list page** at `(app)/+page.svelte` with `+page.ts` load
   function that fetches conversations via `api('/conversations')`.

10. **Build core components:**
    - `ChatMessage.svelte` — message bubble with markdown rendering and tool call display.
    - `ChatInput.svelte` — textarea with Enter-to-send, Shift+Enter for newlines.
    - `StreamingText.svelte` — text content with blinking cursor during streaming.
    - `ToolCallDisplay.svelte` — collapsible panel for tool name, input, output.
    - `ConversationList.svelte` — sidebar list with "New chat" button.

11. **Create chat page** at `(app)/chat/[id]/+page.svelte` with `+page.ts` load function.
    Integrate WebSocket store for real-time streaming. Handle all `WsMessage` types:
    deltas appended to current message, tool calls displayed inline, done signals finalize
    the message, errors shown to user.

12. **Create MCP settings page** at `(app)/settings/mcp/+page.svelte` with `+page.ts`.
    Implement CRUD operations: list servers, add new server, edit existing, toggle
    enabled/disabled, delete with confirmation.

13. **Add `+error.svelte` pages** at root level and within each layout group for graceful
    error boundaries.

14. **Style all pages with Tailwind.** Implement dark mode via `dark:` variant. Make
    sidebar responsive (collapse to hamburger on mobile). Ensure consistent spacing and
    typography.

15. **Run `pnpm check`** to verify TypeScript and Svelte compilation. Fix any type errors
    or warnings.

---

## Acceptance Criteria

- Login form submits to `/auth/login`, sets cookie, redirects to `/`.
- Register form submits to `/auth/register`, shows "pending approval" message on success.
- Unauthenticated users are redirected to `/login` when accessing `(app)` routes.
- Conversation list loads from API and displays with titles and timestamps.
- New conversation creation works via the sidebar button.
- Chat page connects via WebSocket, sends user messages, displays streaming assistant
  response with visible cursor.
- Tool calls display inline in assistant messages with collapsible input/output details.
- MCP server configuration page supports add, edit, toggle, and delete operations.
- `pnpm check` passes with no errors.
- Responsive layout: sidebar collapses on viewports under 768px.
- Dark mode activates based on system preference via Tailwind.

---

## Dependencies

- Phase 002 (SvelteKit scaffold with Tailwind) must be complete.
- Backend auth endpoints (`/auth/me`, `/auth/login`, `/auth/register`) must exist or be
  stubbed.
- Backend conversation endpoints (`/conversations`, `/conversations/:id`) must exist or
  be stubbed.
- Backend WebSocket endpoint (`/ws/:conversationId`) must exist or be stubbed.
- Backend MCP endpoints (`/mcp/servers`) must exist or be stubbed.

---

## Notes

- All components use Svelte 5 patterns exclusively: `$props()`, `$state`, `$derived`,
  `$effect`, callback props (`onclick`), snippets (`{@render}`). No legacy patterns.
- Shared reactive state lives in `.svelte.ts` files, not in legacy Svelte stores.
- The static adapter means no server-side rendering of dynamic content. All API calls
  happen client-side. The `+layout.ts` and `+page.ts` load functions run as universal
  loads (client-side after initial navigation).
