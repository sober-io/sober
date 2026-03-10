<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import type { ToolCall, ServerWsMessage, ConversationWithMessages, Message } from '$lib/types';
	import { websocket } from '$lib/stores/websocket.svelte';
	import ChatMessage from '$lib/components/ChatMessage.svelte';
	import ChatInput from '$lib/components/ChatInput.svelte';

	interface ChatMsg {
		id: string;
		role: 'User' | 'Assistant' | 'System';
		content: string;
		toolCalls?: ToolCall[];
		streaming: boolean;
	}

	interface PageData {
		conversation: ConversationWithMessages;
	}

	let { data }: { data: PageData } = $props();

	function toChat(m: Message): ChatMsg {
		return {
			id: m.id,
			role: m.role,
			content: m.content,
			toolCalls: undefined,
			streaming: false
		};
	}

	const initialMessages = $derived(data.conversation.messages.map(toChat));
	// eslint-disable-next-line svelte/prefer-writable-derived -- messages is mutated by WebSocket handlers
	let messages = $state<ChatMsg[]>([]);
	let streaming = $state(false);
	let messagesContainer: HTMLDivElement | undefined = $state();

	const conversationId = $derived($page.params.id ?? '');

	$effect(() => {
		messages = initialMessages;
	});

	onMount(() => {
		websocket.connect();
	});

	$effect(() => {
		const id = conversationId;
		const unsub = websocket.subscribe(id, handleWsMessage);
		return unsub;
	});

	$effect(() => {
		if (messagesContainer) {
			messagesContainer.scrollTop = messagesContainer.scrollHeight;
		}
	});

	function handleWsMessage(msg: ServerWsMessage) {
		switch (msg.type) {
			case 'chat.delta': {
				const last = messages[messages.length - 1];
				if (last && last.role === 'Assistant' && last.streaming) {
					last.content += msg.content;
				} else {
					messages.push({
						id: crypto.randomUUID(),
						role: 'Assistant',
						content: msg.content,
						streaming: true
					});
				}
				break;
			}
			case 'chat.tool_use': {
				const last = messages[messages.length - 1];
				if (last && last.role === 'Assistant') {
					const tc: ToolCall = {
						name: msg.tool_call.name,
						input: msg.tool_call.input
					};
					last.toolCalls = [...(last.toolCalls ?? []), tc];
				}
				break;
			}
			case 'chat.tool_result': {
				const last = messages[messages.length - 1];
				if (last?.toolCalls) {
					const tc = last.toolCalls.find((t) => t.name && !t.output);
					if (tc) tc.output = msg.output;
				}
				break;
			}
			case 'chat.done': {
				const last = messages[messages.length - 1];
				if (last) {
					last.streaming = false;
					last.id = msg.message_id;
				}
				streaming = false;
				break;
			}
			case 'chat.error': {
				const last = messages[messages.length - 1];
				if (last && last.streaming) {
					last.streaming = false;
					last.content += `\n\nError: ${msg.error}`;
				} else {
					messages.push({
						id: crypto.randomUUID(),
						role: 'System',
						content: `Error: ${msg.error}`,
						streaming: false
					});
				}
				streaming = false;
				break;
			}
		}
	}

	function sendMessage(content: string) {
		messages.push({
			id: crypto.randomUUID(),
			role: 'User',
			content,
			streaming: false
		});
		streaming = true;
		websocket.send({
			type: 'chat.message',
			conversation_id: conversationId,
			content
		});
	}
</script>

<div class="flex h-full flex-col">
	<header class="flex h-14 items-center border-b border-zinc-200 px-4 dark:border-zinc-800">
		<h1 class="text-sm font-medium text-zinc-900 dark:text-zinc-100">
			{data.conversation.title || 'New conversation'}
		</h1>
	</header>

	<div bind:this={messagesContainer} class="flex-1 space-y-4 overflow-y-auto p-4">
		{#each messages as msg (msg.id)}
			<ChatMessage
				role={msg.role}
				content={msg.content}
				toolCalls={msg.toolCalls}
				streaming={msg.streaming}
			/>
		{/each}

		{#if messages.length === 0}
			<div class="flex h-full items-center justify-center">
				<p class="text-sm text-zinc-400 dark:text-zinc-500">Start a conversation</p>
			</div>
		{/if}
	</div>

	<ChatInput onsend={sendMessage} disabled={streaming} />
</div>
