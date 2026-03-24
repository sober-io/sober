# Frontend Test Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add basic test coverage across the frontend — utilities, reactive stores, and Svelte 5 components (70 tests across 11 files).

**Architecture:** Three-tier testing strategy: pure utility tests (no DOM), reactive store tests (`.svelte.test.ts` for rune support), and component tests (`@testing-library/svelte`). All tiers share a common vitest + jsdom foundation configured via `vite.config.ts`.

**Tech Stack:** Vitest 3.2, @testing-library/svelte 5, @testing-library/jest-dom 6, @testing-library/user-event 14, jsdom, Svelte 5 runes

---

## File Map

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `frontend/vite.config.ts` | Add vitest config + svelteTesting plugin |
| Create | `frontend/vitest-setup.ts` | Global jest-dom matchers + jsdom polyfills |
| Modify | `frontend/tsconfig.json` | Add jest-dom types |
| Modify | `frontend/package.json` | New devDependencies (via pnpm add) |
| Create | `frontend/src/lib/utils/api.test.ts` | API client + ApiError tests |
| Create | `frontend/src/lib/utils/markdown.test.ts` | Markdown rendering + XSS tests |
| Create | `frontend/src/lib/utils/time.test.ts` | Relative time formatting tests |
| Create | `frontend/src/lib/stores/auth.svelte.test.ts` | Auth store reactive state tests |
| Create | `frontend/src/lib/stores/conversations.svelte.test.ts` | Conversations store tests |
| Create | `frontend/src/lib/stores/websocket.svelte.test.ts` | WebSocket singleton tests |
| Create | `frontend/src/lib/components/ChatMessage.test.ts` | ChatMessage component tests |
| Create | `frontend/src/lib/components/ChatInput.test.ts` | ChatInput component tests |
| Create | `frontend/src/lib/components/ConversationSettings.test.ts` | ConversationSettings component tests |
| Create | `frontend/src/lib/components/TagInput.test.ts` | TagInput component tests |
| Create | `frontend/src/lib/components/MessageTagPopover.test.ts` | MessageTagPopover component tests |

---

### Task 1: Test Infrastructure Setup

**Files:**
- Modify: `frontend/package.json` (via pnpm)
- Modify: `frontend/vite.config.ts`
- Modify: `frontend/tsconfig.json`
- Create: `frontend/vitest-setup.ts`

- [ ] **Step 1: Install test dependencies**

```bash
cd frontend && pnpm add -D @testing-library/svelte @testing-library/jest-dom @testing-library/user-event jsdom --silent
```

- [ ] **Step 2: Create vitest-setup.ts**

Create `frontend/vitest-setup.ts`:

```ts
import '@testing-library/jest-dom/vitest';

// jsdom doesn't fully implement HTMLDialogElement — polyfill for ConfirmDialog tests
HTMLDialogElement.prototype.showModal ??= function () {
	(this as HTMLDialogElement).open = true;
};
HTMLDialogElement.prototype.close ??= function () {
	(this as HTMLDialogElement).open = false;
};
```

- [ ] **Step 3: Update vite.config.ts**

Replace `frontend/vite.config.ts` with:

```ts
import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { svelteTesting } from '@testing-library/svelte/vite';
import { defineConfig } from 'vitest/config';

export default defineConfig({
	plugins: [tailwindcss(), sveltekit(), svelteTesting()],
	test: {
		environment: 'jsdom',
		setupFiles: ['./vitest-setup.ts'],
		include: ['src/**/*.{test,spec}.{js,ts}', 'src/**/*.svelte.{test,spec}.{js,ts}']
	}
});
```

- [ ] **Step 4: Update tsconfig.json**

Add `"types": ["@testing-library/jest-dom"]` to `compilerOptions` in `frontend/tsconfig.json`:

```json
{
	"extends": "./.svelte-kit/tsconfig.json",
	"compilerOptions": {
		"allowJs": true,
		"checkJs": true,
		"esModuleInterop": true,
		"forceConsistentCasingInFileNames": true,
		"resolveJsonModule": true,
		"skipLibCheck": true,
		"sourceMap": true,
		"strict": true,
		"moduleResolution": "bundler",
		"types": ["@testing-library/jest-dom"]
	}
}
```

- [ ] **Step 5: Verify setup with a smoke test**

Create a temporary file `frontend/src/lib/utils/smoke.test.ts`:

```ts
import { describe, it, expect } from 'vitest';

describe('smoke', () => {
	it('vitest works', () => {
		expect(1 + 1).toBe(2);
	});
});
```

Run: `cd frontend && pnpm test --silent`

Expected: 1 test passes.

Delete the smoke test file after confirming.

- [ ] **Step 6: Move plan to active and commit**

```bash
git mv docs/plans/pending/039-frontend-tests docs/plans/active/039-frontend-tests
git add frontend/vite.config.ts frontend/vitest-setup.ts frontend/tsconfig.json frontend/package.json frontend/pnpm-lock.yaml docs/plans/active/039-frontend-tests
git commit -m "test(frontend): add vitest test infrastructure with @testing-library/svelte"
```

---

### Task 2: API Utility Tests

**Files:**
- Create: `frontend/src/lib/utils/api.test.ts`
- Source: `frontend/src/lib/utils/api.ts`

- [ ] **Step 1: Write tests**

Create `frontend/src/lib/utils/api.test.ts`:

```ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { api, ApiError } from './api';

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

beforeEach(() => {
	mockFetch.mockReset();
});

function jsonResponse(data: unknown, status = 200) {
	return {
		ok: status >= 200 && status < 300,
		status,
		json: () => Promise.resolve(data)
	};
}

describe('api', () => {
	it('unwraps { data: T } envelope on success', async () => {
		mockFetch.mockResolvedValue(jsonResponse({ data: { id: '1', name: 'test' } }));

		const result = await api<{ id: string; name: string }>('/users/1');

		expect(result).toEqual({ id: '1', name: 'test' });
	});

	it('prepends /api/v1 and sets default headers and credentials', async () => {
		mockFetch.mockResolvedValue(jsonResponse({ data: null }));

		await api('/users');

		expect(mockFetch).toHaveBeenCalledWith('/api/v1/users', {
			headers: { 'Content-Type': 'application/json' },
			credentials: 'include'
		});
	});

	it('spreads custom RequestInit options', async () => {
		mockFetch.mockResolvedValue(jsonResponse({ data: null }));

		await api('/auth/login', {
			method: 'POST',
			body: JSON.stringify({ email: 'a@b.com' })
		});

		expect(mockFetch).toHaveBeenCalledWith('/api/v1/auth/login', {
			headers: { 'Content-Type': 'application/json' },
			credentials: 'include',
			method: 'POST',
			body: JSON.stringify({ email: 'a@b.com' })
		});
	});

	it('throws ApiError with status, code, and message on non-ok response', async () => {
		mockFetch.mockResolvedValue({
			ok: false,
			status: 422,
			json: () => Promise.resolve({ error: { code: 'validation', message: 'Invalid email' } })
		});

		const err = await api('/users').catch((e) => e);

		expect(err).toBeInstanceOf(ApiError);
		expect(err.status).toBe(422);
		expect(err.code).toBe('validation');
		expect(err.message).toBe('Invalid email');
	});

	it('falls back to defaults on malformed error body', async () => {
		mockFetch.mockResolvedValue({
			ok: false,
			status: 500,
			json: () => Promise.resolve({ unexpected: true })
		});

		const err = await api('/fail').catch((e) => e);

		expect(err).toBeInstanceOf(ApiError);
		expect(err.status).toBe(500);
		expect(err.code).toBe('unknown');
		expect(err.message).toBe('API error 500');
	});

	it('handles non-JSON error body gracefully', async () => {
		mockFetch.mockResolvedValue({
			ok: false,
			status: 502,
			json: () => Promise.reject(new Error('not json'))
		});

		const err = await api('/fail').catch((e) => e);

		expect(err).toBeInstanceOf(ApiError);
		expect(err.status).toBe(502);
		expect(err.code).toBe('unknown');
		expect(err.message).toBe('API error 502');
	});
});
```

- [ ] **Step 2: Run tests**

Run: `cd frontend && pnpm test --silent -- src/lib/utils/api.test.ts`

Expected: 6 tests pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/utils/api.test.ts
git commit -m "test(frontend): add api utility tests"
```

---

### Task 3: Markdown Utility Tests

**Files:**
- Create: `frontend/src/lib/utils/markdown.test.ts`
- Source: `frontend/src/lib/utils/markdown.ts`

- [ ] **Step 1: Write tests**

Create `frontend/src/lib/utils/markdown.test.ts`:

```ts
import { describe, it, expect } from 'vitest';
import { renderMarkdown } from './markdown';

describe('renderMarkdown', () => {
	it('renders basic markdown formatting', () => {
		const html = renderMarkdown('**bold** _italic_ `code`');

		expect(html).toContain('<strong>bold</strong>');
		expect(html).toContain('<em>italic</em>');
		expect(html).toContain('<code>code</code>');
	});

	it('renders GFM features: tables, strikethrough, line breaks', () => {
		const table = renderMarkdown('| A | B |\n|---|---|\n| 1 | 2 |');
		expect(table).toContain('<table>');
		expect(table).toContain('<td>1</td>');

		const strike = renderMarkdown('~~deleted~~');
		expect(strike).toContain('<del>deleted</del>');
	});

	it('strips script tags (XSS)', () => {
		const html = renderMarkdown('<script>alert("xss")</script>hello');

		expect(html).not.toContain('<script>');
		expect(html).not.toContain('alert');
		expect(html).toContain('hello');
	});

	it('strips forbidden tags: iframe, object, embed, form, style', () => {
		const tests = [
			'<iframe src="evil.com"></iframe>',
			'<object data="x"></object>',
			'<embed src="x">',
			'<form action="/steal"><input></form>',
			'<style>body{display:none}</style>'
		];

		for (const input of tests) {
			const html = renderMarkdown(input);
			expect(html).not.toMatch(/<(iframe|object|embed|form|input|style)/);
		}
	});

	it('preserves allowed tags: tables, images', () => {
		const img = renderMarkdown('![alt text](https://example.com/img.png "title")');
		expect(img).toContain('<img');
		expect(img).toContain('src="https://example.com/img.png"');
		expect(img).toContain('alt="alt text"');
	});

	it('blocks data-* attributes', () => {
		const html = renderMarkdown('<div data-evil="payload">content</div>');

		expect(html).not.toContain('data-evil');
		expect(html).toContain('content');
	});
});
```

- [ ] **Step 2: Run tests**

Run: `cd frontend && pnpm test --silent -- src/lib/utils/markdown.test.ts`

Expected: 6 tests pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/utils/markdown.test.ts
git commit -m "test(frontend): add markdown rendering and XSS sanitization tests"
```

---

### Task 4: Time Utility Tests

**Files:**
- Create: `frontend/src/lib/utils/time.test.ts`
- Source: `frontend/src/lib/utils/time.ts`

- [ ] **Step 1: Write tests**

Create `frontend/src/lib/utils/time.test.ts`:

```ts
import { describe, it, expect } from 'vitest';
import { formatRelativeTime, formatRelativeFuture } from './time';

describe('time utils', () => {
	it('formatRelativeTime returns human-readable past time', () => {
		const twoHoursAgo = new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString();
		const result = formatRelativeTime(twoHoursAgo);

		expect(result).toBe('2 hours ago');
	});

	it('formatRelativeFuture returns human-readable future time', () => {
		const inThreeDays = new Date(Date.now() + 3 * 24 * 60 * 60 * 1000).toISOString();
		const result = formatRelativeFuture(inThreeDays);

		expect(result).toBe('in 3 days');
	});
});
```

- [ ] **Step 2: Run tests**

Run: `cd frontend && pnpm test --silent -- src/lib/utils/time.test.ts`

Expected: 2 tests pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/utils/time.test.ts
git commit -m "test(frontend): add time formatting utility tests"
```

---

### Task 5: Auth Store Tests

**Files:**
- Create: `frontend/src/lib/stores/auth.svelte.test.ts`
- Source: `frontend/src/lib/stores/auth.svelte.ts`

Note: `.svelte.test.ts` extension is required for the Svelte compiler to transform runes (`$state`, `$derived`).

- [ ] **Step 1: Write tests**

Create `frontend/src/lib/stores/auth.svelte.test.ts`:

```ts
import { describe, it, expect } from 'vitest';
import { auth } from './auth.svelte';

describe('auth store', () => {
	it('has correct initial state', () => {
		// Reset to initial state
		auth.setUser(null);
		auth.setLoading(true);

		expect(auth.user).toBeNull();
		expect(auth.loading).toBe(true);
		expect(auth.isAuthenticated).toBe(false);
	});

	it('setUser updates user and isAuthenticated becomes true', () => {
		auth.setUser({ id: '1', email: 'a@b.com', username: 'alice', status: 'active' });

		expect(auth.user).toEqual({ id: '1', email: 'a@b.com', username: 'alice', status: 'active' });
		expect(auth.isAuthenticated).toBe(true);
	});

	it('setUser(null) resets isAuthenticated to false', () => {
		auth.setUser({ id: '1', email: 'a@b.com', username: 'alice', status: 'active' });
		expect(auth.isAuthenticated).toBe(true);

		auth.setUser(null);
		expect(auth.isAuthenticated).toBe(false);
	});

	it('setLoading updates loading state', () => {
		auth.setLoading(false);
		expect(auth.loading).toBe(false);

		auth.setLoading(true);
		expect(auth.loading).toBe(true);
	});
});
```

- [ ] **Step 2: Run tests**

Run: `cd frontend && pnpm test --silent -- src/lib/stores/auth.svelte.test.ts`

Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/stores/auth.svelte.test.ts
git commit -m "test(frontend): add auth store reactive state tests"
```

---

### Task 6: Conversations Store Tests

**Files:**
- Create: `frontend/src/lib/stores/conversations.svelte.test.ts`
- Source: `frontend/src/lib/stores/conversations.svelte.ts`

- [ ] **Step 1: Write tests**

Create `frontend/src/lib/stores/conversations.svelte.test.ts`:

```ts
import { describe, it, expect, beforeEach } from 'vitest';
import { conversations } from './conversations.svelte';
import type { Conversation, Tag } from '$lib/types';

function makeConversation(overrides: Partial<Conversation> = {}): Conversation {
	return {
		id: crypto.randomUUID(),
		title: 'Test',
		kind: 'direct',
		is_archived: false,
		permission_mode: 'interactive',
		agent_mode: 'always',
		unread_count: 0,
		tags: [],
		created_at: '2026-01-01T00:00:00Z',
		updated_at: '2026-01-01T00:00:00Z',
		...overrides
	};
}

describe('conversations store', () => {
	beforeEach(() => {
		conversations.set([]);
	});

	it('set replaces the items list', () => {
		const list = [makeConversation({ id: 'a' }), makeConversation({ id: 'b' })];
		conversations.set(list);

		expect(conversations.items).toHaveLength(2);
		expect(conversations.items[0].id).toBe('a');
	});

	it('prepend adds conversation to front', () => {
		conversations.set([makeConversation({ id: 'existing' })]);
		conversations.prepend(makeConversation({ id: 'new' }));

		expect(conversations.items[0].id).toBe('new');
		expect(conversations.items).toHaveLength(2);
	});

	it('updateTitle updates matching conversation', () => {
		conversations.set([makeConversation({ id: 'a', title: 'Old' })]);
		conversations.updateTitle('a', 'New Title');

		expect(conversations.items[0].title).toBe('New Title');
	});

	it('remove filters out conversation by id', () => {
		conversations.set([makeConversation({ id: 'a' }), makeConversation({ id: 'b' })]);
		conversations.remove('a');

		expect(conversations.items).toHaveLength(1);
		expect(conversations.items[0].id).toBe('b');
	});

	it('updateUnread sets count and sorts unread first, then by updated_at', () => {
		const old = makeConversation({ id: 'old', updated_at: '2026-01-01T00:00:00Z', unread_count: 0 });
		const recent = makeConversation({ id: 'recent', updated_at: '2026-03-01T00:00:00Z', unread_count: 0 });
		conversations.set([old, recent]);

		// Mark the older one as unread — it should jump to front
		conversations.updateUnread('old', 3);

		expect(conversations.items[0].id).toBe('old');
		expect(conversations.items[0].unread_count).toBe(3);
		expect(conversations.items[1].id).toBe('recent');
	});

	it('updateUnread sort stability: two unread items sorted by updated_at', () => {
		const older = makeConversation({ id: 'older', updated_at: '2026-01-01T00:00:00Z', unread_count: 1 });
		const newer = makeConversation({ id: 'newer', updated_at: '2026-03-01T00:00:00Z', unread_count: 1 });
		const read = makeConversation({ id: 'read', updated_at: '2026-02-01T00:00:00Z', unread_count: 0 });
		conversations.set([older, newer, read]);

		// Trigger sort by updating any unread
		conversations.updateUnread('older', 2);

		expect(conversations.items[0].id).toBe('newer');
		expect(conversations.items[1].id).toBe('older');
		expect(conversations.items[2].id).toBe('read');
	});

	it('markRead sets unread_count to 0', () => {
		conversations.set([makeConversation({ id: 'a', unread_count: 5 })]);
		conversations.markRead('a');

		expect(conversations.items[0].unread_count).toBe(0);
	});

	it('archive and unarchive toggle is_archived', () => {
		conversations.set([makeConversation({ id: 'a', is_archived: false })]);

		conversations.archive('a');
		expect(conversations.items[0].is_archived).toBe(true);

		conversations.unarchive('a');
		expect(conversations.items[0].is_archived).toBe(false);
	});

	it('updateTags replaces tags on matching conversation', () => {
		conversations.set([makeConversation({ id: 'a', tags: [] })]);

		const newTags: Tag[] = [{ id: 't1', name: 'bug', color: '#f00', created_at: '2026-01-01T00:00:00Z' }];
		conversations.updateTags('a', newTags);

		expect(conversations.items[0].tags).toEqual(newTags);
	});

	it('update merges partial fields into matching conversation', () => {
		conversations.set([makeConversation({ id: 'a', title: 'Old', kind: 'direct' })]);
		conversations.update('a', { title: 'Updated', kind: 'group' });

		expect(conversations.items[0].title).toBe('Updated');
		expect(conversations.items[0].kind).toBe('group');
	});
});
```

- [ ] **Step 2: Run tests**

Run: `cd frontend && pnpm test --silent -- src/lib/stores/conversations.svelte.test.ts`

Expected: 10 tests pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/stores/conversations.svelte.test.ts
git commit -m "test(frontend): add conversations store tests including sort logic"
```

---

### Task 7: WebSocket Store Tests

**Files:**
- Create: `frontend/src/lib/stores/websocket.svelte.test.ts`
- Source: `frontend/src/lib/stores/websocket.svelte.ts`

This test requires mocking `WebSocket`, `location`, and timers. The websocket store is a singleton that persists between tests, so each test connects and disconnects cleanly.

- [ ] **Step 1: Write tests**

Create `frontend/src/lib/stores/websocket.svelte.test.ts`:

```ts
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Mock the conversations store before importing websocket (it imports conversations)
vi.mock('$lib/stores/conversations.svelte', () => ({
	conversations: {
		updateUnread: vi.fn()
	}
}));

// Mock WebSocket
class MockWebSocket {
	static readonly OPEN = 1;
	static readonly CLOSED = 3;
	static instances: MockWebSocket[] = [];

	url: string;
	readyState = MockWebSocket.OPEN;
	onopen: (() => void) | null = null;
	onclose: (() => void) | null = null;
	onerror: (() => void) | null = null;
	onmessage: ((e: { data: string }) => void) | null = null;
	send = vi.fn();
	close = vi.fn(() => {
		this.readyState = MockWebSocket.CLOSED;
	});

	constructor(url: string) {
		this.url = url;
		MockWebSocket.instances.push(this);
	}

	/** Simulate the server accepting the connection. */
	simulateOpen() {
		this.readyState = MockWebSocket.OPEN;
		this.onopen?.();
	}

	/** Simulate the connection closing. */
	simulateClose() {
		this.readyState = MockWebSocket.CLOSED;
		this.onclose?.();
	}
}

vi.stubGlobal('WebSocket', MockWebSocket);
vi.stubGlobal('location', { protocol: 'http:', host: 'localhost:8080' });

// Import after mocks are in place
const { websocket } = await import('./websocket.svelte');

describe('websocket store', () => {
	beforeEach(() => {
		vi.useFakeTimers();
		MockWebSocket.instances = [];
		websocket.disconnect();
	});

	afterEach(() => {
		websocket.disconnect();
		vi.useRealTimers();
	});

	it('has correct initial state', () => {
		expect(websocket.connected).toBe(false);
		expect(websocket.error).toBeNull();
	});

	it('queues messages when disconnected and flushes on connect', () => {
		const msg = { type: 'chat.message' as const, conversation_id: 'c1', content: 'hello' };
		websocket.send(msg);

		// Not connected — message should be queued, not sent
		expect(MockWebSocket.instances).toHaveLength(0);

		// Now connect
		websocket.connect();
		const ws = MockWebSocket.instances[0];
		ws.simulateOpen();

		// Queued message should have been flushed
		const calls = ws.send.mock.calls.map((c) => JSON.parse(c[0] as string));
		expect(calls).toContainEqual(msg);
	});

	it('subscribe registers handler and sends chat.subscribe', () => {
		websocket.connect();
		const ws = MockWebSocket.instances[0];
		ws.simulateOpen();

		const handler = vi.fn();
		websocket.subscribe('conv-1', handler);

		// Should have sent a subscribe message
		const calls = ws.send.mock.calls.map((c) => {
			try { return JSON.parse(c[0] as string); } catch { return c[0]; }
		});
		expect(calls).toContainEqual({ type: 'chat.subscribe', conversation_id: 'conv-1' });

		// Simulate incoming message for that conversation
		ws.onmessage?.({ data: JSON.stringify({ type: 'chat.delta', conversation_id: 'conv-1', content: 'hi' }) });
		expect(handler).toHaveBeenCalledWith({ type: 'chat.delta', conversation_id: 'conv-1', content: 'hi' });
	});

	it('unsubscribe removes handler', () => {
		websocket.connect();
		const ws = MockWebSocket.instances[0];
		ws.simulateOpen();

		const handler = vi.fn();
		const unsub = websocket.subscribe('conv-1', handler);
		unsub();

		// Message should not reach handler after unsubscribe
		ws.onmessage?.({ data: JSON.stringify({ type: 'chat.delta', conversation_id: 'conv-1', content: 'hi' }) });
		expect(handler).not.toHaveBeenCalled();
	});

	it('disconnect clears all state', () => {
		websocket.connect();
		const ws = MockWebSocket.instances[0];
		ws.simulateOpen();
		expect(websocket.connected).toBe(true);

		websocket.disconnect();
		expect(websocket.connected).toBe(false);
		expect(ws.close).toHaveBeenCalled();
	});

	it('schedules reconnect with increasing delay after onclose', () => {
		websocket.connect();
		const ws1 = MockWebSocket.instances[0];
		ws1.simulateOpen();
		ws1.simulateClose();

		// First reconnect at 1000ms
		expect(MockWebSocket.instances).toHaveLength(1); // No new instance yet
		vi.advanceTimersByTime(1000);
		expect(MockWebSocket.instances).toHaveLength(2); // Reconnected

		// Second close + reconnect at 2000ms
		const ws2 = MockWebSocket.instances[1];
		ws2.simulateClose();
		vi.advanceTimersByTime(1500); // Not enough
		expect(MockWebSocket.instances).toHaveLength(2);
		vi.advanceTimersByTime(500); // Now 2000ms total
		expect(MockWebSocket.instances).toHaveLength(3);
	});
});
```

- [ ] **Step 2: Run tests**

Run: `cd frontend && pnpm test --silent -- src/lib/stores/websocket.svelte.test.ts`

Expected: 6 tests pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/stores/websocket.svelte.test.ts
git commit -m "test(frontend): add websocket store tests with mock WebSocket"
```

---

### Task 8: ChatMessage Component Tests

**Files:**
- Create: `frontend/src/lib/components/ChatMessage.test.ts`
- Source: `frontend/src/lib/components/ChatMessage.svelte`

- [ ] **Step 1: Write tests**

Create `frontend/src/lib/components/ChatMessage.test.ts`:

```ts
import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { userEvent } from '@testing-library/user-event';
import ChatMessage from './ChatMessage.svelte';

describe('ChatMessage', () => {
	it('renders user message right-aligned', () => {
		const { container } = render(ChatMessage, {
			props: { role: 'user', content: 'Hello' }
		});

		const wrapper = container.querySelector('.justify-end');
		expect(wrapper).toBeInTheDocument();
	});

	it('renders assistant message left-aligned', () => {
		const { container } = render(ChatMessage, {
			props: { role: 'assistant', content: 'Hi there' }
		});

		const wrapper = container.querySelector('.justify-start');
		expect(wrapper).toBeInTheDocument();
	});

	it('renders event role as centered italic text', () => {
		render(ChatMessage, {
			props: { role: 'event', content: 'User joined' }
		});

		const el = screen.getByText('User joined');
		expect(el.tagName).toBe('SPAN');
		expect(el.classList.contains('italic')).toBe(true);
	});

	it('renders content as sanitized markdown HTML', () => {
		const { container } = render(ChatMessage, {
			props: { role: 'assistant', content: '**bold text**' }
		});

		const strong = container.querySelector('strong');
		expect(strong).toBeInTheDocument();
		expect(strong?.textContent).toBe('bold text');
	});

	it('shows thinking indicator when thinking with no content', () => {
		render(ChatMessage, {
			props: { role: 'assistant', content: '', thinking: true }
		});

		expect(screen.getByRole('status', { name: /thinking/i })).toBeInTheDocument();
	});

	it('shows streaming text when streaming', () => {
		render(ChatMessage, {
			props: { role: 'assistant', content: 'Streaming...', streaming: true }
		});

		expect(screen.getByText('Streaming...')).toBeInTheDocument();
	});

	it('displays tool calls when provided', () => {
		render(ChatMessage, {
			props: {
				role: 'assistant',
				content: 'Done.',
				toolCalls: [{ id: 'tc1', name: 'search', input: { query: 'test' }, output: 'found it' }]
			}
		});

		expect(screen.getByText('search')).toBeInTheDocument();
	});

	it('renders first 2 tags and shows "+N more" button when 3+ tags', () => {
		const tags = [
			{ id: '1', name: 'bug', color: '#f00', created_at: '2026-01-01T00:00:00Z' },
			{ id: '2', name: 'feat', color: '#0f0', created_at: '2026-01-01T00:00:00Z' },
			{ id: '3', name: 'docs', color: '#00f', created_at: '2026-01-01T00:00:00Z' }
		];

		render(ChatMessage, {
			props: { role: 'assistant', content: 'test', tags }
		});

		expect(screen.getByText('bug')).toBeInTheDocument();
		expect(screen.getByText('feat')).toBeInTheDocument();
		expect(screen.queryByText('docs')).not.toBeInTheDocument();
		expect(screen.getByText('+1 more')).toBeInTheDocument();
	});

	it('clicking "+N more" reveals all tags', async () => {
		const user = userEvent.setup();
		const tags = [
			{ id: '1', name: 'bug', color: '#f00', created_at: '2026-01-01T00:00:00Z' },
			{ id: '2', name: 'feat', color: '#0f0', created_at: '2026-01-01T00:00:00Z' },
			{ id: '3', name: 'docs', color: '#00f', created_at: '2026-01-01T00:00:00Z' }
		];

		render(ChatMessage, {
			props: { role: 'assistant', content: 'test', tags }
		});

		await user.click(screen.getByText('+1 more'));

		expect(screen.getByText('docs')).toBeInTheDocument();
		expect(screen.queryByText('+1 more')).not.toBeInTheDocument();
	});

	it('hides action bar during streaming, ephemeral, and thinking', () => {
		const { container } = render(ChatMessage, {
			props: { role: 'assistant', content: 'test', streaming: true }
		});

		// Action bar has opacity-0 and group-hover:opacity-100
		// When canShowActions is false, the bar is not rendered at all
		const actionBar = container.querySelector('[class*="group-hover"]');
		expect(actionBar).not.toBeInTheDocument();
	});
});
```

- [ ] **Step 2: Run tests**

Run: `cd frontend && pnpm test --silent -- src/lib/components/ChatMessage.test.ts`

Expected: 10 tests pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/components/ChatMessage.test.ts
git commit -m "test(frontend): add ChatMessage component tests"
```

---

### Task 9: ChatInput Component Tests

**Files:**
- Create: `frontend/src/lib/components/ChatInput.test.ts`
- Source: `frontend/src/lib/components/ChatInput.svelte`

- [ ] **Step 1: Write tests**

Create `frontend/src/lib/components/ChatInput.test.ts`:

```ts
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { userEvent } from '@testing-library/user-event';
import ChatInput from './ChatInput.svelte';

describe('ChatInput', () => {
	it('send button is disabled when input is empty', () => {
		render(ChatInput, {
			props: { onsend: vi.fn() }
		});

		const button = screen.getByRole('button', { name: /send/i });
		expect(button).toBeDisabled();
	});

	it('Enter key submits trimmed value and clears input', async () => {
		const user = userEvent.setup();
		const onsend = vi.fn();
		render(ChatInput, { props: { onsend } });

		const textarea = screen.getByPlaceholderText(/send a message/i);
		await user.type(textarea, 'hello world');
		await user.keyboard('{Enter}');

		expect(onsend).toHaveBeenCalledWith('hello world');
	});

	it('Shift+Enter does not submit', async () => {
		const user = userEvent.setup();
		const onsend = vi.fn();
		render(ChatInput, { props: { onsend } });

		const textarea = screen.getByPlaceholderText(/send a message/i);
		await user.type(textarea, 'hello');
		await user.keyboard('{Shift>}{Enter}{/Shift}');

		expect(onsend).not.toHaveBeenCalled();
	});

	it('slash prefix shows palette, slash+space hides it', async () => {
		const user = userEvent.setup();
		render(ChatInput, {
			props: { onsend: vi.fn(), skills: [{ name: 'test-skill', description: 'A test skill' }] }
		});

		const textarea = screen.getByPlaceholderText(/send a message/i);

		// Typing "/" should trigger the palette
		await user.type(textarea, '/');
		// SlashCommandPalette renders command items
		expect(screen.getByText('/help')).toBeInTheDocument();

		// Adding a space hides the palette (value becomes "/ ")
		await user.type(textarea, ' ');
		expect(screen.queryByText('/help')).not.toBeInTheDocument();
	});

	it('builtin commands trigger onSlashCommand', async () => {
		const user = userEvent.setup();
		const onSlashCommand = vi.fn();
		render(ChatInput, {
			props: { onsend: vi.fn(), onSlashCommand }
		});

		const textarea = screen.getByPlaceholderText(/send a message/i);
		// Type a complete builtin command and submit
		await user.clear(textarea);
		await user.type(textarea, '/help');
		// Submit by pressing Enter — but palette is open so Enter is intercepted.
		// Instead simulate clicking Send button after palette closes.
		// Actually, let's type the command with a space so palette closes, then submit.
		await user.clear(textarea);
		await user.type(textarea, '/help ');
		await user.keyboard('{Enter}');

		// /help with space — first word is /help which is a builtin
		expect(onSlashCommand).toHaveBeenCalledWith('/help');
	});

	it('non-builtin slash commands go through onsend', async () => {
		const user = userEvent.setup();
		const onsend = vi.fn();
		render(ChatInput, {
			props: { onsend, onSlashCommand: vi.fn() }
		});

		const textarea = screen.getByPlaceholderText(/send a message/i);
		await user.clear(textarea);
		await user.type(textarea, '/my-skill do something');
		await user.keyboard('{Enter}');

		expect(onsend).toHaveBeenCalledWith('/my-skill do something');
	});

	it('shows "Queue" when busy', () => {
		render(ChatInput, {
			props: { onsend: vi.fn(), busy: true }
		});

		expect(screen.getByText('Queue')).toBeInTheDocument();
	});
});
```

- [ ] **Step 2: Run tests**

Run: `cd frontend && pnpm test --silent -- src/lib/components/ChatInput.test.ts`

Expected: 7 tests pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/components/ChatInput.test.ts
git commit -m "test(frontend): add ChatInput component tests"
```

---

### Task 10: ConversationSettings Component Tests

**Files:**
- Create: `frontend/src/lib/components/ConversationSettings.test.ts`
- Source: `frontend/src/lib/components/ConversationSettings.svelte`

The component calls `conversationService.listCollaborators` and `jobService.listByConversation` via `$effect` on open. Both services must be mocked.

- [ ] **Step 1: Write tests**

Create `frontend/src/lib/components/ConversationSettings.test.ts`:

```ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import { userEvent } from '@testing-library/user-event';
import ConversationSettings from './ConversationSettings.svelte';
import type { Conversation, PermissionMode, Tag } from '$lib/types';

vi.mock('$lib/services/conversations', () => ({
	conversationService: {
		listCollaborators: vi.fn().mockResolvedValue([]),
		addCollaborator: vi.fn(),
		removeCollaborator: vi.fn(),
		updateCollaboratorRole: vi.fn(),
		updateAgentMode: vi.fn(),
		convertToGroup: vi.fn(),
		leave: vi.fn()
	}
}));

vi.mock('$lib/services/jobs', () => ({
	jobService: {
		listByConversation: vi.fn().mockResolvedValue([])
	}
}));

vi.mock('$lib/stores/auth.svelte', () => ({
	auth: {
		get user() {
			return { id: 'user-1', email: 'a@b.com', username: 'alice', status: 'active' };
		}
	}
}));

vi.mock('$lib/stores/conversations.svelte', () => ({
	conversations: {
		update: vi.fn()
	}
}));

function makeConversation(overrides: Partial<Conversation> = {}): Conversation {
	return {
		id: 'conv-1',
		title: 'Test Conversation',
		kind: 'direct',
		is_archived: false,
		permission_mode: 'interactive',
		agent_mode: 'always',
		unread_count: 0,
		tags: [],
		created_at: '2026-01-15T00:00:00Z',
		updated_at: '2026-01-15T00:00:00Z',
		...overrides
	};
}

const defaultProps = {
	open: true,
	conversation: makeConversation(),
	tags: [] as Tag[],
	permissionMode: 'interactive' as PermissionMode,
	onClose: vi.fn(),
	onUpdateTitle: vi.fn(),
	onUpdatePermissionMode: vi.fn(),
	onAddTag: vi.fn(),
	onRemoveTag: vi.fn(),
	onArchive: vi.fn(),
	onClearHistory: vi.fn(),
	onDelete: vi.fn()
};

describe('ConversationSettings', () => {
	beforeEach(() => {
		vi.clearAllMocks();
	});

	it('renders nothing when open is false', () => {
		const { container } = render(ConversationSettings, {
			props: { ...defaultProps, open: false }
		});

		expect(container.querySelector('[role="dialog"]')).not.toBeInTheDocument();
	});

	it('renders panel with title, kind label, and created date when open', async () => {
		render(ConversationSettings, { props: defaultProps });

		await waitFor(() => {
			expect(screen.getByRole('dialog')).toBeInTheDocument();
		});
		expect(screen.getByText('Settings')).toBeInTheDocument();
		expect(screen.getByText('Direct')).toBeInTheDocument();
		expect(screen.getByText(/2026/)).toBeInTheDocument();
	});

	it('calls onUpdateTitle on blur with trimmed value', async () => {
		const user = userEvent.setup();
		const onUpdateTitle = vi.fn();
		render(ConversationSettings, {
			props: { ...defaultProps, onUpdateTitle }
		});

		const input = screen.getByPlaceholderText('Conversation title...');
		await user.clear(input);
		await user.type(input, '  New Title  ');
		await user.tab(); // triggers blur

		expect(onUpdateTitle).toHaveBeenCalledWith('New Title');
	});

	it('archive button text toggles based on is_archived', async () => {
		const { rerender } = render(ConversationSettings, {
			props: defaultProps
		});

		await waitFor(() => {
			expect(screen.getByText('Archive conversation')).toBeInTheDocument();
		});

		rerender({ ...defaultProps, conversation: makeConversation({ is_archived: true }) });

		await waitFor(() => {
			expect(screen.getByText('Unarchive conversation')).toBeInTheDocument();
		});
	});

	it('clear history button opens confirm dialog', async () => {
		const user = userEvent.setup();
		render(ConversationSettings, { props: defaultProps });

		await user.click(screen.getByText('Clear message history'));

		expect(screen.getByText('All messages in this conversation will be permanently deleted. This action cannot be undone.')).toBeInTheDocument();
	});

	it('delete button hidden for inbox kind', () => {
		render(ConversationSettings, {
			props: { ...defaultProps, conversation: makeConversation({ kind: 'inbox' }) }
		});

		expect(screen.queryByText('Delete conversation')).not.toBeInTheDocument();
	});

	it('delete confirm dialog calls onDelete', async () => {
		const user = userEvent.setup();
		const onDelete = vi.fn();
		render(ConversationSettings, {
			props: { ...defaultProps, onDelete }
		});

		await user.click(screen.getByText('Delete conversation'));
		await user.click(screen.getByText('Delete'));

		expect(onDelete).toHaveBeenCalled();
	});

	it('close button calls onClose', async () => {
		const user = userEvent.setup();
		const onClose = vi.fn();
		render(ConversationSettings, {
			props: { ...defaultProps, onClose }
		});

		await user.click(screen.getByLabelText('Close'));

		expect(onClose).toHaveBeenCalled();
	});
});
```

- [ ] **Step 2: Run tests**

Run: `cd frontend && pnpm test --silent -- src/lib/components/ConversationSettings.test.ts`

Expected: 8 tests pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/components/ConversationSettings.test.ts
git commit -m "test(frontend): add ConversationSettings component tests"
```

---

### Task 11: TagInput Component Tests

**Files:**
- Create: `frontend/src/lib/components/TagInput.test.ts`
- Source: `frontend/src/lib/components/TagInput.svelte`

- [ ] **Step 1: Write tests**

Create `frontend/src/lib/components/TagInput.test.ts`:

```ts
import { describe, it, expect, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import { userEvent } from '@testing-library/user-event';
import TagInput from './TagInput.svelte';
import type { Tag } from '$lib/types';

vi.mock('$lib/services/tags', () => ({
	tagService: {
		list: vi.fn().mockResolvedValue([
			{ id: 's1', name: 'bug', color: '#f00', created_at: '2026-01-01T00:00:00Z' },
			{ id: 's2', name: 'feature', color: '#0f0', created_at: '2026-01-01T00:00:00Z' },
			{ id: 's3', name: 'docs', color: '#00f', created_at: '2026-01-01T00:00:00Z' }
		])
	}
}));

const existingTags: Tag[] = [
	{ id: 't1', name: 'bug', color: '#f00', created_at: '2026-01-01T00:00:00Z' }
];

describe('TagInput', () => {
	it('renders existing tags with remove buttons', () => {
		render(TagInput, {
			props: { tags: existingTags, onAdd: vi.fn(), onRemove: vi.fn() }
		});

		expect(screen.getByText('bug')).toBeInTheDocument();
		expect(screen.getByLabelText('Remove tag bug')).toBeInTheDocument();
	});

	it('"Add tag" button reveals input field', async () => {
		const user = userEvent.setup();
		render(TagInput, {
			props: { tags: [], onAdd: vi.fn(), onRemove: vi.fn() }
		});

		await user.click(screen.getByText('Add tag'));

		expect(screen.getByPlaceholderText('Tag name…')).toBeInTheDocument();
	});

	it('Enter submits new tag via onAdd callback', async () => {
		const user = userEvent.setup();
		const onAdd = vi.fn();
		render(TagInput, {
			props: { tags: [], onAdd, onRemove: vi.fn() }
		});

		await user.click(screen.getByText('Add tag'));
		const input = screen.getByPlaceholderText('Tag name…');
		await user.type(input, 'new-tag');
		await user.keyboard('{Enter}');

		expect(onAdd).toHaveBeenCalledWith('new-tag');
	});

	it('Escape closes input and clears value', async () => {
		const user = userEvent.setup();
		render(TagInput, {
			props: { tags: [], onAdd: vi.fn(), onRemove: vi.fn() }
		});

		await user.click(screen.getByText('Add tag'));
		const input = screen.getByPlaceholderText('Tag name…');
		await user.type(input, 'draft');
		await user.keyboard('{Escape}');

		expect(screen.queryByPlaceholderText('Tag name…')).not.toBeInTheDocument();
	});

	it('remove button calls onRemove with tag id', async () => {
		const user = userEvent.setup();
		const onRemove = vi.fn();
		render(TagInput, {
			props: { tags: existingTags, onAdd: vi.fn(), onRemove }
		});

		await user.click(screen.getByLabelText('Remove tag bug'));

		expect(onRemove).toHaveBeenCalledWith('t1');
	});

	it('suggestions exclude already-applied tags', async () => {
		const user = userEvent.setup();
		render(TagInput, {
			props: { tags: existingTags, onAdd: vi.fn(), onRemove: vi.fn() }
		});

		await user.click(screen.getByText('Add tag'));
		const input = screen.getByPlaceholderText('Tag name…');
		await user.click(input); // focus to show suggestions

		// "bug" is already applied — should not appear in suggestions
		// "feature" and "docs" should appear (wait for async tagService.list)
		await waitFor(() => {
			const suggestions = screen.queryAllByRole('button').filter((b) => b.closest('ul'));
			const suggestionTexts = suggestions.map((s) => s.textContent?.trim());
			expect(suggestionTexts).not.toContain('bug');
		});
	});
});
```

- [ ] **Step 2: Run tests**

Run: `cd frontend && pnpm test --silent -- src/lib/components/TagInput.test.ts`

Expected: 6 tests pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/components/TagInput.test.ts
git commit -m "test(frontend): add TagInput component tests"
```

---

### Task 12: MessageTagPopover Component Tests

**Files:**
- Create: `frontend/src/lib/components/MessageTagPopover.test.ts`
- Source: `frontend/src/lib/components/MessageTagPopover.svelte`

- [ ] **Step 1: Write tests**

Create `frontend/src/lib/components/MessageTagPopover.test.ts`:

```ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import { userEvent } from '@testing-library/user-event';
import MessageTagPopover from './MessageTagPopover.svelte';
import type { Tag } from '$lib/types';

const mockTagService = {
	list: vi.fn(),
	addToMessage: vi.fn(),
	removeFromMessage: vi.fn()
};

vi.mock('$lib/services/tags', () => ({
	tagService: mockTagService
}));

const allTags: Tag[] = [
	{ id: 's1', name: 'bug', color: '#f00', created_at: '2026-01-01T00:00:00Z' },
	{ id: 's2', name: 'feature', color: '#0f0', created_at: '2026-01-01T00:00:00Z' },
	{ id: 's3', name: 'docs', color: '#00f', created_at: '2026-01-01T00:00:00Z' }
];

const appliedTags: Tag[] = [allTags[0]]; // bug is applied

describe('MessageTagPopover', () => {
	beforeEach(() => {
		vi.clearAllMocks();
		mockTagService.list.mockResolvedValue(allTags);
	});

	it('renders existing tags with remove buttons', () => {
		render(MessageTagPopover, {
			props: { messageId: 'msg-1', tags: appliedTags, onTagsChange: vi.fn(), onClose: vi.fn() }
		});

		expect(screen.getByText('bug')).toBeInTheDocument();
		expect(screen.getByLabelText('Remove tag bug')).toBeInTheDocument();
	});

	it('filters suggestions by input text (case-insensitive)', async () => {
		const user = userEvent.setup();
		render(MessageTagPopover, {
			props: { messageId: 'msg-1', tags: [], onTagsChange: vi.fn(), onClose: vi.fn() }
		});

		const input = await screen.findByPlaceholderText('Add tag…');
		await user.type(input, 'FEA');

		await waitFor(() => {
			const suggestions = screen.queryAllByRole('button').filter((b) => b.closest('ul'));
			const texts = suggestions.map((s) => s.textContent?.trim());
			expect(texts).toContain('feature');
			expect(texts).not.toContain('docs');
		});
	});

	it('excludes already-applied tags from suggestions', async () => {
		render(MessageTagPopover, {
			props: { messageId: 'msg-1', tags: appliedTags, onTagsChange: vi.fn(), onClose: vi.fn() }
		});

		// Wait for suggestions to load
		await waitFor(() => {
			const suggestions = screen.queryAllByRole('button').filter((b) => b.closest('ul'));
			const texts = suggestions.map((s) => s.textContent?.trim());
			expect(texts).not.toContain('bug');
		});
	});

	it('clicking suggestion adds tag and fires onTagsChange', async () => {
		const user = userEvent.setup();
		const onTagsChange = vi.fn();
		const newTag = { id: 'new-1', name: 'feature', color: '#0f0', created_at: '2026-01-01T00:00:00Z' };
		mockTagService.addToMessage.mockResolvedValue(newTag);

		render(MessageTagPopover, {
			props: { messageId: 'msg-1', tags: appliedTags, onTagsChange, onClose: vi.fn() }
		});

		// Wait for suggestions to appear
		const featureBtn = await screen.findByRole('button', { name: /feature/i });
		await user.click(featureBtn);

		await waitFor(() => {
			expect(mockTagService.addToMessage).toHaveBeenCalledWith('msg-1', 'feature');
			expect(onTagsChange).toHaveBeenCalledWith([...appliedTags, newTag]);
		});
	});

	it('Escape key calls onClose', async () => {
		const user = userEvent.setup();
		const onClose = vi.fn();
		render(MessageTagPopover, {
			props: { messageId: 'msg-1', tags: [], onTagsChange: vi.fn(), onClose }
		});

		const input = await screen.findByPlaceholderText('Add tag…');
		await user.type(input, '{Escape}');

		expect(onClose).toHaveBeenCalled();
	});
});
```

- [ ] **Step 2: Run tests**

Run: `cd frontend && pnpm test --silent -- src/lib/components/MessageTagPopover.test.ts`

Expected: 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/components/MessageTagPopover.test.ts
git commit -m "test(frontend): add MessageTagPopover component tests"
```

---

### Task 13: Full Test Suite Verification

- [ ] **Step 1: Run entire test suite**

```bash
cd frontend && pnpm test --silent
```

Expected: All 70 tests pass across 11 test files.

- [ ] **Step 2: Run type checking**

```bash
cd frontend && pnpm check
```

Expected: No type errors.

- [ ] **Step 3: Run linting**

```bash
cd frontend && pnpm lint
```

Expected: No lint errors. Fix any formatting issues with `pnpm format`.

- [ ] **Step 4: Final commit if formatting changes needed**

```bash
cd frontend && pnpm format
git add -u
git commit -m "style(frontend): format test files"
```

---

### Task 14: Move Plan to Done and Bump Version

- [ ] **Step 1: Bump frontend version (patch bump for test-only change)**

Update `version` in `frontend/package.json` from `"0.9.0"` to `"0.9.1"`.

- [ ] **Step 2: Move plan to done and commit**

```bash
git mv docs/plans/active/039-frontend-tests docs/plans/done/039-frontend-tests
git add docs/plans/done/039-frontend-tests frontend/package.json
git commit -m "test(frontend): #039 add test coverage — 70 tests across utilities, stores, and components"
```
