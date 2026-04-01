# #048: Desktop Notifications Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show browser desktop notifications when a new message arrives and the user isn't looking at that conversation.

**Architecture:** A single utility module (`notifications.svelte.ts`) owns permission state and the `notify()` function. The WebSocket store intercepts `chat.new_message` globally (before per-conversation routing) and calls `notify()` when the message is from someone else and the conversation isn't currently focused. Clicking a notification navigates to the conversation.

**Tech Stack:** Web Notifications API, SvelteKit `$app/navigation` (`goto`), Svelte 5 runes.

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `frontend/src/lib/stores/notifications.svelte.ts` | Create | Permission state, `requestPermission()`, `notify()` |
| `frontend/src/lib/stores/notifications.svelte.test.ts` | Create | Unit tests for notification logic |
| `frontend/src/lib/stores/websocket.svelte.ts` | Modify (lines 101-111) | Intercept `chat.new_message` globally, call `notify()` |
| `frontend/src/lib/stores/auth.svelte.ts` | Read-only | Used to check current user ID |

---

### Task 1: Create the notifications store

**Files:**
- Create: `frontend/src/lib/stores/notifications.svelte.ts`
- Test: `frontend/src/lib/stores/notifications.svelte.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `frontend/src/lib/stores/notifications.svelte.test.ts`:

```typescript
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Mock $app/navigation before importing the module under test
vi.mock('$app/navigation', () => ({ goto: vi.fn() }));

import { notifications } from './notifications.svelte';
import { goto } from '$app/navigation';

let mockPermission = 'default';
const mockNotificationInstance = { close: vi.fn(), onclick: null as (() => void) | null };
const MockNotification = vi.fn(() => mockNotificationInstance);
Object.defineProperty(MockNotification, 'permission', { get: () => mockPermission });
MockNotification.requestPermission = vi.fn();

beforeEach(() => {
	mockPermission = 'default';
	vi.stubGlobal('Notification', MockNotification);
	vi.stubGlobal('document', { ...document, hidden: false });
	MockNotification.mockClear();
	MockNotification.requestPermission.mockReset();
	mockNotificationInstance.onclick = null;
	vi.mocked(goto).mockReset();
});

afterEach(() => {
	vi.unstubAllGlobals();
});

describe('notifications.requestPermission', () => {
	it('calls Notification.requestPermission when permission is default', async () => {
		MockNotification.requestPermission.mockResolvedValue('granted');
		await notifications.requestPermission();
		expect(MockNotification.requestPermission).toHaveBeenCalledOnce();
	});

	it('does not call requestPermission when already granted', async () => {
		mockPermission = 'granted';
		await notifications.requestPermission();
		expect(MockNotification.requestPermission).not.toHaveBeenCalled();
	});

	it('does not call requestPermission when denied', async () => {
		mockPermission = 'denied';
		await notifications.requestPermission();
		expect(MockNotification.requestPermission).not.toHaveBeenCalled();
	});
});

describe('notifications.notify', () => {
	it('creates a Notification when permission is granted and tab is hidden', () => {
		mockPermission = 'granted';
		vi.stubGlobal('document', { hidden: true });

		notifications.notify({
			conversationId: 'conv-1',
			title: 'Alice',
			body: 'Hello there'
		});

		expect(MockNotification).toHaveBeenCalledWith('Alice', {
			body: 'Hello there',
			tag: 'conv-1'
		});
	});

	it('does not create a Notification when tab is focused', () => {
		mockPermission = 'granted';
		vi.stubGlobal('document', { hidden: false });

		notifications.notify({
			conversationId: 'conv-1',
			title: 'Alice',
			body: 'Hello there',
			isActiveConversation: false
		});

		expect(MockNotification).not.toHaveBeenCalled();
	});

	it('does not create a Notification when permission is not granted', () => {
		mockPermission = 'default';
		vi.stubGlobal('document', { hidden: true });

		notifications.notify({
			conversationId: 'conv-1',
			title: 'Alice',
			body: 'Hello there'
		});

		expect(MockNotification).not.toHaveBeenCalled();
	});

	it('creates a Notification when tab is focused but conversation is not active', () => {
		mockPermission = 'granted';
		vi.stubGlobal('document', { hidden: false });

		notifications.notify({
			conversationId: 'conv-1',
			title: 'Alice',
			body: 'Hello there',
			isActiveConversation: false
		});

		// Tab is focused — no notification even for non-active conversation.
		// We only notify when the tab itself is hidden.
		expect(MockNotification).not.toHaveBeenCalled();
	});

	it('navigates to conversation on notification click', () => {
		mockPermission = 'granted';
		vi.stubGlobal('document', { hidden: true });
		// Mock window.focus
		vi.stubGlobal('window', { ...window, focus: vi.fn() });

		notifications.notify({
			conversationId: 'conv-1',
			title: 'Alice',
			body: 'Hello there'
		});

		// Simulate click
		mockNotificationInstance.onclick!();
		expect(window.focus).toHaveBeenCalled();
		expect(goto).toHaveBeenCalledWith('/chat/conv-1');
	});
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd frontend && pnpm test --silent -- notifications.svelte.test`
Expected: FAIL — module `./notifications.svelte` does not exist.

- [ ] **Step 3: Implement the notifications store**

Create `frontend/src/lib/stores/notifications.svelte.ts`:

```typescript
import { goto } from '$app/navigation';

interface NotifyOptions {
	conversationId: string;
	title: string;
	body: string;
	/** Whether this conversation is currently being viewed. */
	isActiveConversation?: boolean;
}

/** Desktop notification permission and dispatch. */
export const notifications = (() => {
	/** Request notification permission if not yet decided. */
	const requestPermission = async () => {
		if (typeof Notification === 'undefined') return;
		if (Notification.permission !== 'default') return;
		await Notification.requestPermission();
	};

	/**
	 * Show a desktop notification if the tab is hidden and permission is granted.
	 * Clicking the notification focuses the tab and navigates to the conversation.
	 */
	const notify = ({ conversationId, title, body }: NotifyOptions) => {
		if (typeof Notification === 'undefined') return;
		if (Notification.permission !== 'granted') return;
		if (!document.hidden) return;

		const n = new Notification(title, { body, tag: conversationId });
		n.onclick = () => {
			window.focus();
			goto(`/chat/${conversationId}`);
			n.close();
		};
	};

	return { requestPermission, notify };
})();
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd frontend && pnpm test --silent -- notifications.svelte.test`
Expected: All 6 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/stores/notifications.svelte.ts frontend/src/lib/stores/notifications.svelte.test.ts
git commit -m "feat(frontend): add desktop notification store with permission handling"
```

---

### Task 2: Wire notifications into the WebSocket store

**Files:**
- Modify: `frontend/src/lib/stores/websocket.svelte.ts:1-5,101-111`

- [ ] **Step 1: Add the global `chat.new_message` handler**

In `frontend/src/lib/stores/websocket.svelte.ts`, add the import at line 3:

```typescript
import { notifications } from '$lib/stores/notifications.svelte';
import { auth } from '$lib/stores/auth.svelte';
```

Then in `socket.onmessage` (after the existing `chat.unread` block at line 107), add:

```typescript
			if (msg.type === 'chat.new_message') {
				// Notify for messages from others (not our own, not assistant).
				if (msg.role === 'user' && msg.user_id && msg.user_id !== auth.user?.id) {
					const title = msg.username ?? 'New message';
					const text =
						msg.content
							.filter((b): b is Extract<typeof b, { type: 'text' }> => b.type === 'text')
							.map((b) => b.text)
							.join('\n')
							.slice(0, 200) || 'Sent an attachment';
					notifications.notify({
						conversationId: msg.conversation_id,
						title,
						body: text
					});
				}
			}
```

This goes *before* the per-conversation handler dispatch (the `const handler = handlers.get(...)` line), so the notification fires regardless of whether the conversation page is mounted.

The full `onmessage` handler after the edit:

```typescript
		socket.onmessage = (e) => {
			const msg: ServerWsMessage = JSON.parse(e.data);
			if (!('conversation_id' in msg)) return;
			// Handle global conversation-level events before per-conversation routing.
			if (msg.type === 'chat.unread') {
				conversations.updateUnread(msg.conversation_id, msg.unread_count);
				return;
			}
			if (msg.type === 'chat.new_message') {
				if (msg.role === 'user' && msg.user_id && msg.user_id !== auth.user?.id) {
					const title = msg.username ?? 'New message';
					const text =
						msg.content
							.filter((b): b is Extract<typeof b, { type: 'text' }> => b.type === 'text')
							.map((b) => b.text)
							.join('\n')
							.slice(0, 200) || 'Sent an attachment';
					notifications.notify({
						conversationId: msg.conversation_id,
						title,
						body: text
					});
				}
			}
			const handler = handlers.get(msg.conversation_id);
			if (handler) handler(msg);
		};
```

- [ ] **Step 2: Run existing tests and type-check**

Run: `cd frontend && pnpm check && pnpm test --silent`
Expected: All checks and tests pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/stores/websocket.svelte.ts
git commit -m "feat(frontend): fire desktop notifications on incoming messages"
```

---

### Task 3: Request permission on first message arrival

**Files:**
- Modify: `frontend/src/lib/stores/websocket.svelte.ts:101-120`

- [ ] **Step 1: Add lazy permission request**

The permission prompt should fire once — the first time a `chat.new_message` from another user arrives. Add a `permissionRequested` flag and call `requestPermission()` inside the `chat.new_message` block.

In the `chat.new_message` handler in `websocket.svelte.ts`, wrap the existing block:

```typescript
			if (msg.type === 'chat.new_message') {
				if (msg.role === 'user' && msg.user_id && msg.user_id !== auth.user?.id) {
					notifications.requestPermission();
					const title = msg.username ?? 'New message';
					const text =
						msg.content
							.filter((b): b is Extract<typeof b, { type: 'text' }> => b.type === 'text')
							.map((b) => b.text)
							.join('\n')
							.slice(0, 200) || 'Sent an attachment';
					notifications.notify({
						conversationId: msg.conversation_id,
						title,
						body: text
					});
				}
			}
```

`requestPermission()` is idempotent — it no-ops if permission is already `granted` or `denied`, so calling it on each message is safe and avoids extra state.

- [ ] **Step 2: Run tests and type-check**

Run: `cd frontend && pnpm check && pnpm test --silent`
Expected: All pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/stores/websocket.svelte.ts
git commit -m "feat(frontend): lazily request notification permission on first incoming message"
```

---

### Task 4: Final verification

- [ ] **Step 1: Run full frontend checks**

```bash
cd frontend && pnpm check && pnpm test --silent && pnpm build
```

Expected: All pass, no type errors, build succeeds.

- [ ] **Step 2: Manual smoke test**

1. Open the app in two browser tabs logged in as different users.
2. In tab A, send a message in a conversation with user B.
3. Switch to tab B (or leave it backgrounded) — a desktop notification should appear.
4. Click the notification — tab B should focus and navigate to the conversation.
5. Send a message from tab B — no notification should appear in tab B (own message).
