import { SvelteMap, SvelteSet } from 'svelte/reactivity';
import type { ClientWsMessage, ServerWsMessage } from '$lib/types';

type MessageHandler = (data: ServerWsMessage) => void;

/** How long to wait before reconnecting (ms). */
const RECONNECT_DELAYS = [1000, 2000, 5000, 10000];

/** Interval between client-side pings to keep the connection alive (ms). */
const PING_INTERVAL = 30_000;

/** Singleton reactive WebSocket connection to /api/v1/ws. */
export const websocket = (() => {
	let ws = $state<WebSocket | null>(null);
	let connected = $state(false);
	let error = $state<string | null>(null);
	let intentionalClose = false;
	let reconnectAttempt = 0;
	let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	let pingTimer: ReturnType<typeof setInterval> | null = null;
	const handlers = new SvelteMap<string, MessageHandler>();

	/** Conversation IDs with active subscriptions — re-sent on reconnect. */
	const subscribedConversations = new SvelteSet<string>();

	/** Messages queued while the WebSocket is not yet open. */
	let pendingQueue: ClientWsMessage[] = [];

	const stopPing = () => {
		if (pingTimer) {
			clearInterval(pingTimer);
			pingTimer = null;
		}
	};

	const startPing = (socket: WebSocket) => {
		stopPing();
		pingTimer = setInterval(() => {
			if (socket.readyState === WebSocket.OPEN) {
				socket.send('ping');
			}
		}, PING_INTERVAL);
	};

	/** Sends raw message on the socket. Only call when socket is OPEN. */
	const sendRaw = (socket: WebSocket, msg: ClientWsMessage) => {
		socket.send(JSON.stringify(msg));
	};

	/** Flushes queued messages and re-subscribes active conversations. */
	const flushOnOpen = (socket: WebSocket) => {
		// Re-register all active conversation subscriptions with the backend.
		for (const conversationId of subscribedConversations) {
			sendRaw(socket, { type: 'chat.subscribe', conversation_id: conversationId });
		}

		// Flush any messages queued while disconnected.
		const queue = pendingQueue;
		pendingQueue = [];
		for (const msg of queue) {
			sendRaw(socket, msg);
		}
	};

	const scheduleReconnect = () => {
		if (intentionalClose) return;
		const delay = RECONNECT_DELAYS[Math.min(reconnectAttempt, RECONNECT_DELAYS.length - 1)];
		reconnectAttempt++;
		reconnectTimer = setTimeout(() => {
			reconnectTimer = null;
			connectInner();
		}, delay);
	};

	const connectInner = () => {
		if (ws && ws.readyState !== WebSocket.CLOSED) return;

		const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
		const socket = new WebSocket(`${protocol}//${location.host}/api/v1/ws`);

		socket.onopen = () => {
			connected = true;
			error = null;
			reconnectAttempt = 0;
			startPing(socket);
			flushOnOpen(socket);
		};

		socket.onclose = () => {
			connected = false;
			ws = null;
			stopPing();
			scheduleReconnect();
		};

		socket.onerror = () => {
			error = 'Connection lost';
		};

		socket.onmessage = (e) => {
			const msg: ServerWsMessage = JSON.parse(e.data);
			if (!('conversation_id' in msg)) return;
			const handler = handlers.get(msg.conversation_id);
			if (handler) handler(msg);
		};

		ws = socket;
	};

	const connect = () => {
		intentionalClose = false;
		reconnectAttempt = 0;
		connectInner();
	};

	const disconnect = () => {
		intentionalClose = true;
		if (reconnectTimer) {
			clearTimeout(reconnectTimer);
			reconnectTimer = null;
		}
		stopPing();
		ws?.close();
		ws = null;
		connected = false;
		handlers.clear();
		subscribedConversations.clear();
		pendingQueue = [];
	};

	const send = (msg: ClientWsMessage) => {
		if (ws && ws.readyState === WebSocket.OPEN) {
			ws.send(JSON.stringify(msg));
		} else {
			pendingQueue.push(msg);
		}
	};

	/** Subscribe to messages for a specific conversation. Returns an unsubscribe function. */
	const subscribe = (conversationId: string, handler: MessageHandler): (() => void) => {
		handlers.set(conversationId, handler);
		subscribedConversations.add(conversationId);

		// Send chat.subscribe immediately if connected, otherwise it will
		// be sent when the connection opens (via flushOnOpen).
		send({ type: 'chat.subscribe', conversation_id: conversationId });

		return () => {
			handlers.delete(conversationId);
			subscribedConversations.delete(conversationId);
		};
	};

	return {
		get connected() {
			return connected;
		},
		get error() {
			return error;
		},
		connect,
		disconnect,
		send,
		subscribe
	};
})();
