import { SvelteMap } from 'svelte/reactivity';
import type { ClientWsMessage, ServerWsMessage } from '$lib/types';

type MessageHandler = (data: ServerWsMessage) => void;

/** Singleton reactive WebSocket connection to /api/v1/ws. */
export const websocket = (() => {
	let ws = $state<WebSocket | null>(null);
	let connected = $state(false);
	let error = $state<string | null>(null);
	const handlers = new SvelteMap<string, MessageHandler>();

	const connect = () => {
		if (ws) return;
		const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
		const socket = new WebSocket(`${protocol}//${location.host}/api/v1/ws`);

		socket.onopen = () => {
			connected = true;
			error = null;
		};

		socket.onclose = () => {
			connected = false;
			ws = null;
		};

		socket.onerror = () => {
			error = 'Connection lost';
		};

		socket.onmessage = (e) => {
			const msg: ServerWsMessage = JSON.parse(e.data);
			const handler = handlers.get(msg.conversation_id);
			if (handler) handler(msg);
		};

		ws = socket;
	};

	const disconnect = () => {
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
