import { SvelteMap } from 'svelte/reactivity';
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
	};

	const send = (msg: ClientWsMessage) => {
		ws?.send(JSON.stringify(msg));
	};

	/** Subscribe to messages for a specific conversation. Returns an unsubscribe function. */
	const subscribe = (conversationId: string, handler: MessageHandler): (() => void) => {
		handlers.set(conversationId, handler);
		return () => {
			handlers.delete(conversationId);
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
