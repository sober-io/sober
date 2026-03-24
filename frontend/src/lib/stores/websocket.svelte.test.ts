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
			try {
				return JSON.parse(c[0] as string);
			} catch {
				return c[0];
			}
		});
		expect(calls).toContainEqual({ type: 'chat.subscribe', conversation_id: 'conv-1' });

		// Simulate incoming message for that conversation
		ws.onmessage?.({
			data: JSON.stringify({ type: 'chat.delta', conversation_id: 'conv-1', content: 'hi' })
		});
		expect(handler).toHaveBeenCalledWith({
			type: 'chat.delta',
			conversation_id: 'conv-1',
			content: 'hi'
		});
	});

	it('unsubscribe removes handler', () => {
		websocket.connect();
		const ws = MockWebSocket.instances[0];
		ws.simulateOpen();

		const handler = vi.fn();
		const unsub = websocket.subscribe('conv-1', handler);
		unsub();

		// Message should not reach handler after unsubscribe
		ws.onmessage?.({
			data: JSON.stringify({ type: 'chat.delta', conversation_id: 'conv-1', content: 'hi' })
		});
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
