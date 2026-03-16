<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { page } from '$app/stores';
	import type {
		ToolCall,
		ServerWsMessage,
		Conversation,
		Message,
		ConfirmRequest,
		PermissionMode,
		Tag
	} from '$lib/types';
	import { websocket } from '$lib/stores/websocket.svelte';
	import { conversations } from '$lib/stores/conversations.svelte';
	import { conversationService } from '$lib/services/conversations';
	import { tagService } from '$lib/services/tags';
	import ChatMessage from '$lib/components/ChatMessage.svelte';
	import ChatInput from '$lib/components/ChatInput.svelte';
	import ScrollToBottom from '$lib/components/ScrollToBottom.svelte';
	import ConfirmationCard from '$lib/components/chat/ConfirmationCard.svelte';
	import StatusBar from '$lib/components/chat/StatusBar.svelte';
	import TagInput from '$lib/components/TagInput.svelte';

	interface ChatMsg {
		id: string;
		role: 'User' | 'Assistant' | 'System';
		content: string;
		thinkingContent: string;
		toolCalls?: ToolCall[];
		streaming: boolean;
		thinking: boolean;
		timestamp: string;
		source?: string;
	}

	interface QueuedMessage {
		id: string;
		content: string;
	}

	interface PageData {
		conversation: Conversation;
		messages: Message[];
	}

	type AssistantPhase = 'idle' | 'thinking' | 'streaming';

	let { data }: { data: PageData } = $props();

	const fmtTime = (iso?: string) => {
		const d = iso ? new Date(iso) : new Date();
		return d.toLocaleTimeString([], {
			hour: '2-digit',
			minute: '2-digit',
			second: '2-digit',
			hour12: false
		});
	};

	const toChat = (m: Message): ChatMsg => ({
		id: m.id,
		role: m.role === 'Tool' ? 'System' : m.role,
		content: m.content,
		thinkingContent: '',
		toolCalls: undefined,
		streaming: false,
		thinking: false,
		timestamp: fmtTime(m.created_at)
	});

	let messages = $state<ChatMsg[]>([]);
	let loadingMore = $state(false);
	let allLoaded = $state(data.messages.length < 50);
	let sentinel: HTMLDivElement | undefined = $state();
	let assistantPhase = $state<AssistantPhase>('idle');
	const isBusy = $derived(assistantPhase !== 'idle');

	let messageQueue = $state<QueuedMessage[]>([]);
	let editingQueueId = $state<string | null>(null);
	let editingContent = $state('');

	let isAtBottom = $state(true);
	let messagesContainer: HTMLDivElement | undefined = $state();
	let title = $state(data.conversation.title || '');
	let tags = $state<Tag[]>(data.conversation.tags ?? []);
	let editingTitle = $state(false);
	let editTitleValue = $state('');
	let pendingConfirms = $state<ConfirmRequest[]>([]);
	let permissionMode = $state<PermissionMode>('policy_based');

	const conversationId = $derived($page.params.id ?? '');

	// Reset state when conversation changes
	$effect(() => {
		void conversationId;
		messages = data.messages.map(toChat);
		allLoaded = data.messages.length < 50;
		assistantPhase = 'idle';
		messageQueue = [];
		editingQueueId = null;
		isAtBottom = true;
		title = data.conversation.title || '';
		tags = data.conversation.tags ?? [];
		pendingConfirms = [];
		permissionMode = data.conversation.permission_mode ?? 'policy_based';
		conversations.markRead(data.conversation.id);
	});

	// Scroll to bottom on conversation change
	$effect(() => {
		void conversationId;
		if (messagesContainer) {
			requestAnimationFrame(() => {
				messagesContainer!.scrollTop = messagesContainer!.scrollHeight;
			});
		}
	});

	onMount(() => {
		websocket.connect();
	});

	$effect(() => {
		const id = conversationId;
		// subscribe() sends chat.subscribe to the backend (queued if not yet
		// connected) and re-sends it automatically on reconnect.
		return websocket.subscribe(id, handleWsMessage);
	});

	// Auto-scroll when at bottom and messages or confirms change
	$effect(() => {
		void messages.length;
		void pendingConfirms.length;
		const last = messages[messages.length - 1];
		void last?.content;
		if (isAtBottom && messagesContainer) {
			requestAnimationFrame(() => {
				messagesContainer!.scrollTop = messagesContainer!.scrollHeight;
			});
		}
	});

	const handleScroll = () => {
		if (!messagesContainer) return;
		const { scrollHeight, scrollTop, clientHeight } = messagesContainer;
		isAtBottom = scrollHeight - scrollTop - clientHeight < 50;
	};

	const scrollToBottom = () => {
		if (!messagesContainer) return;
		messagesContainer.scrollTo({ top: messagesContainer.scrollHeight, behavior: 'smooth' });
		isAtBottom = true;
	};

	const loadMore = async () => {
		if (loadingMore || allLoaded) return;
		loadingMore = true;
		const oldest = messages[0];
		if (!oldest) {
			loadingMore = false;
			return;
		}
		const container = messagesContainer;
		const older = await conversationService.listMessages(data.conversation.id, oldest.id);
		if (older.length < 50) allLoaded = true;
		if (older.length > 0 && container) {
			const prevHeight = container.scrollHeight;
			messages = [...older.map(toChat), ...messages];
			await tick();
			container.scrollTop = container.scrollHeight - prevHeight;
		}
		loadingMore = false;
	};

	// IntersectionObserver on sentinel div to trigger loadMore
	$effect(() => {
		const el = sentinel;
		if (!el) return;
		const observer = new IntersectionObserver((entries) => {
			if (entries[0].isIntersecting && !allLoaded) {
				loadMore();
			}
		});
		observer.observe(el);
		return () => observer.disconnect();
	});

	const dispatchMessage = (content: string) => {
		const now = fmtTime();
		messages.push({
			id: crypto.randomUUID(),
			role: 'User',
			content,
			thinkingContent: '',
			streaming: false,
			thinking: false,
			timestamp: now
		});
		messages.push({
			id: crypto.randomUUID(),
			role: 'Assistant',
			content: '',
			thinkingContent: '',
			streaming: false,
			thinking: true,
			timestamp: now
		});
		assistantPhase = 'thinking';
		websocket.send({
			type: 'chat.message',
			conversation_id: conversationId,
			content
		});
	};

	const flushQueue = () => {
		if (messageQueue.length > 0) {
			const next = messageQueue.shift()!;
			dispatchMessage(next.content);
		}
	};

	const sendMessage = (content: string) => {
		if (isBusy) {
			messageQueue.push({ id: crypto.randomUUID(), content });
		} else {
			dispatchMessage(content);
		}
	};

	const removeQueued = (id: string) => {
		const idx = messageQueue.findIndex((m) => m.id === id);
		if (idx !== -1) messageQueue.splice(idx, 1);
		if (editingQueueId === id) editingQueueId = null;
	};

	const startEditQueued = (msg: QueuedMessage) => {
		editingQueueId = msg.id;
		editingContent = msg.content;
	};

	const saveEditQueued = (id: string) => {
		const msg = messageQueue.find((m) => m.id === id);
		if (msg) {
			const trimmed = editingContent.trim();
			if (trimmed) {
				msg.content = trimmed;
			} else {
				removeQueued(id);
			}
		}
		editingQueueId = null;
	};

	const handleWsMessage = (msg: ServerWsMessage) => {
		switch (msg.type) {
			case 'chat.thinking': {
				const last = messages[messages.length - 1];
				if (last && last.role === 'Assistant' && (last.thinking || last.streaming)) {
					last.thinkingContent += msg.content;
				}
				break;
			}
			case 'chat.delta': {
				const last = messages[messages.length - 1];
				if (last && last.role === 'Assistant' && (last.thinking || last.streaming)) {
					last.thinking = false;
					last.streaming = true;
					last.content += msg.content;
					assistantPhase = 'streaming';
				} else {
					messages.push({
						id: crypto.randomUUID(),
						role: 'Assistant',
						content: msg.content,
						thinkingContent: '',
						streaming: true,
						thinking: false,
						timestamp: fmtTime()
					});
					assistantPhase = 'streaming';
				}
				break;
			}
			case 'chat.tool_use': {
				const last = messages[messages.length - 1];
				if (last && last.role === 'Assistant') {
					const tc: ToolCall = {
						id: crypto.randomUUID(),
						name: msg.tool_call.name,
						input: msg.tool_call.input
					};
					last.toolCalls = [...(last.toolCalls ?? []), tc];
					last.streaming = true;
					last.thinking = false;
					messages = [...messages];
				}
				break;
			}
			case 'chat.tool_result': {
				const last = messages[messages.length - 1];
				if (last?.toolCalls) {
					const tc = last.toolCalls.find((t) => !t.output);
					if (tc) {
						tc.output = msg.output;
						messages = [...messages];
					}
				}
				break;
			}
			case 'chat.new_message': {
				// If an assistant message is actively streaming, update its
				// ID and source from the stored-message notification.
				const active = messages[messages.length - 1];
				if (active?.role === 'Assistant' && (active.streaming || active.thinking)) {
					active.id = msg.message_id;
					if (msg.source && msg.source !== 'human') active.source = msg.source;
					break;
				}
				// Also check the last completed assistant message (done
				// already fired before new_message arrived).
				const prev = messages[messages.length - 1];
				if (prev?.role === 'Assistant' && !prev.streaming && !prev.thinking) {
					if (msg.source && msg.source !== 'human') prev.source = msg.source;
					break;
				}
				// Otherwise this is a new message from another source (scheduler).
				messages.push({
					id: msg.message_id,
					role: msg.role as ChatMsg['role'],
					content: msg.content,
					thinkingContent: '',
					streaming: false,
					thinking: false,
					timestamp: fmtTime(),
					source: msg.source
				});
				break;
			}
			case 'chat.done': {
				const last = messages[messages.length - 1];
				if (last) {
					last.streaming = false;
					last.thinking = false;
					last.id = msg.message_id;
				}
				assistantPhase = 'idle';
				flushQueue();
				break;
			}
			case 'chat.title': {
				title = msg.title;
				conversations.updateTitle(conversationId, msg.title);
				break;
			}
			case 'chat.confirm': {
				pendingConfirms = [
					...pendingConfirms,
					{
						confirm_id: msg.confirm_id,
						command: msg.command,
						risk_level: msg.risk_level as 'safe' | 'moderate' | 'dangerous',
						affects: msg.affects,
						reason: msg.reason
					}
				];
				break;
			}
			case 'chat.error': {
				const last = messages[messages.length - 1];
				if (last && (last.streaming || last.thinking)) {
					last.streaming = false;
					last.thinking = false;
					last.content += `\n\nError: ${msg.error}`;
				} else {
					messages.push({
						id: crypto.randomUUID(),
						role: 'System',
						content: `Error: ${msg.error}`,
						thinkingContent: '',
						streaming: false,
						thinking: false,
						timestamp: fmtTime()
					});
				}
				assistantPhase = 'idle';
				flushQueue();
				break;
			}
		}
	};

	const handleModeChange = (newMode: PermissionMode) => {
		permissionMode = newMode;
		// Update agent's runtime permission mode via WebSocket.
		websocket.send({
			type: 'chat.set_permission_mode',
			conversation_id: conversationId,
			mode: newMode
		});
		// Persist to conversation.
		conversationService.updatePermissionMode(conversationId, newMode);
	};

	const handleConfirmResponse = (confirmId: string, approved: boolean) => {
		websocket.send({
			type: 'chat.confirm_response',
			conversation_id: conversationId,
			confirm_id: confirmId,
			approved
		});
		// Remove from pending — toast disappears on response.
		pendingConfirms = pendingConfirms.filter((c) => c.confirm_id !== confirmId);
	};

	const handleAddTag = async (name: string) => {
		const tag = await tagService.addToConversation(data.conversation.id, name);
		// Optimistic: avoid duplicates
		if (!tags.some((t) => t.id === tag.id)) {
			tags = [...tags, tag];
		}
	};

	const handleRemoveTag = async (tagId: string) => {
		tags = tags.filter((t) => t.id !== tagId);
		await tagService.removeFromConversation(data.conversation.id, tagId);
	};

	const startEditTitle = () => {
		editTitleValue = title || '';
		editingTitle = true;
	};

	const saveTitle = async () => {
		const trimmed = editTitleValue.trim();
		if (trimmed && trimmed !== title) {
			title = trimmed;
			conversations.updateTitle(conversationId, trimmed);
			await conversationService.updateTitle(conversationId, trimmed);
		}
		editingTitle = false;
	};
</script>

<div class="flex h-full flex-col">
	<header
		class="flex flex-col justify-center gap-1.5 border-b border-zinc-200 px-4 py-2 dark:border-zinc-800"
	>
		<div class="flex items-center">
			{#if editingTitle}
				<input
					type="text"
					bind:value={editTitleValue}
					onkeydown={(e) => {
						if (e.key === 'Enter') saveTitle();
						if (e.key === 'Escape') editingTitle = false;
					}}
					onblur={saveTitle}
					class="w-full rounded border border-zinc-300 bg-transparent px-2 py-1 text-sm font-medium text-zinc-900 outline-none focus:border-zinc-500 dark:border-zinc-700 dark:text-zinc-100 dark:focus:border-zinc-500"
				/>
			{:else}
				<button
					onclick={startEditTitle}
					class="text-sm font-medium text-zinc-900 hover:text-zinc-600 dark:text-zinc-100 dark:hover:text-zinc-400"
					title="Click to rename"
				>
					{title || 'New conversation'}
				</button>
			{/if}
		</div>
		<TagInput {tags} onAdd={handleAddTag} onRemove={handleRemoveTag} />
	</header>

	<div class="relative flex-1 overflow-hidden">
		<div
			bind:this={messagesContainer}
			onscroll={handleScroll}
			class="h-full space-y-4 overflow-y-auto p-4"
		>
			<div bind:this={sentinel}></div>

			{#if loadingMore}
				<div class="flex justify-center py-2">
					<div
						class="h-4 w-4 animate-spin rounded-full border-2 border-zinc-300 border-t-zinc-600 dark:border-zinc-600 dark:border-t-zinc-300"
					></div>
				</div>
			{/if}

			{#each messages as msg (msg.id)}
				<ChatMessage
					role={msg.role}
					content={msg.content}
					thinkingContent={msg.thinkingContent}
					toolCalls={msg.toolCalls}
					streaming={msg.streaming}
					thinking={msg.thinking}
					timestamp={msg.timestamp}
					source={msg.source}
				/>
			{/each}

			{#if messageQueue.length > 0}
				<div class="space-y-2 opacity-50">
					{#each messageQueue as qmsg (qmsg.id)}
						<div class="flex justify-end">
							<div
								class="flex max-w-[80%] items-start gap-2 rounded-lg bg-zinc-900 px-4 py-2 text-sm text-white dark:bg-zinc-100 dark:text-zinc-900"
							>
								{#if editingQueueId === qmsg.id}
									<textarea
										bind:value={editingContent}
										onkeydown={(e) => {
											if (e.key === 'Enter' && !e.shiftKey) {
												e.preventDefault();
												saveEditQueued(qmsg.id);
											}
											if (e.key === 'Escape') editingQueueId = null;
										}}
										rows="1"
										class="flex-1 resize-none bg-transparent outline-none"
									></textarea>
									<button
										onclick={() => saveEditQueued(qmsg.id)}
										class="shrink-0 text-xs underline"
									>
										Save
									</button>
								{:else}
									<div class="flex-1">
										<div class="mb-1 text-[10px] font-medium uppercase tracking-wide opacity-60">
											Queued
										</div>
										<div class="whitespace-pre-wrap">{qmsg.content}</div>
									</div>
									<div class="flex shrink-0 gap-1">
										<button
											onclick={() => startEditQueued(qmsg)}
											class="text-xs underline opacity-70 hover:opacity-100"
										>
											Edit
										</button>
										<button
											onclick={() => removeQueued(qmsg.id)}
											class="text-xs underline opacity-70 hover:opacity-100"
										>
											Remove
										</button>
									</div>
								{/if}
							</div>
						</div>
					{/each}
				</div>
			{/if}

			{#if messages.length === 0 && messageQueue.length === 0}
				<div class="flex h-full items-center justify-center">
					<p class="text-sm text-zinc-400 dark:text-zinc-500">Start a conversation</p>
				</div>
			{/if}
		</div>

		<ScrollToBottom onclick={scrollToBottom} visible={!isAtBottom} />
	</div>

	{#if pendingConfirms.length > 0}
		<div
			class="flex flex-col items-center gap-2 border-t border-zinc-200 px-4 py-3 dark:border-zinc-800"
		>
			{#each pendingConfirms as confirm (confirm.confirm_id)}
				<div class="confirm-grow w-full">
					<ConfirmationCard request={confirm} onRespond={handleConfirmResponse} />
				</div>
			{/each}
		</div>
	{/if}

	<ChatInput onsend={sendMessage} busy={isBusy} />
	<StatusBar mode={permissionMode} onModeChange={handleModeChange} />
</div>

<style>
	.confirm-grow {
		animation: grow-up 0.25s ease-out;
		transform-origin: bottom center;
	}

	@keyframes grow-up {
		from {
			opacity: 0;
			transform: scaleY(0);
			max-height: 0;
		}
		to {
			opacity: 1;
			transform: scaleY(1);
			max-height: 300px;
		}
	}
</style>
