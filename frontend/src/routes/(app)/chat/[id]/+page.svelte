<script lang="ts">
	import { onMount, tick, untrack } from 'svelte';
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { uuid } from '$lib/utils/id';
	import type {
		ToolExecution,
		ServerWsMessage,
		Conversation,
		Message,
		ConfirmRequest,
		PermissionMode,
		Tag
	} from '$lib/types';
	import { websocket } from '$lib/stores/websocket.svelte';
	import { conversations } from '$lib/stores/conversations.svelte';
	import { auth } from '$lib/stores/auth.svelte';
	import { conversationService } from '$lib/services/conversations';
	import { tagService } from '$lib/services/tags';
	import { skillsService } from '$lib/services/skills';
	import type { SkillInfo } from '$lib/types';
	import ChatMessage from '$lib/components/ChatMessage.svelte';
	import ChatInput from '$lib/components/ChatInput.svelte';
	import ScrollToBottom from '$lib/components/ScrollToBottom.svelte';
	import ConfirmationCard from '$lib/components/chat/ConfirmationCard.svelte';
	import StatusBar from '$lib/components/chat/StatusBar.svelte';
	import TagInput from '$lib/components/TagInput.svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import ConversationSettings from '$lib/components/ConversationSettings.svelte';

	interface ChatMsg {
		id: string;
		role: 'user' | 'assistant' | 'system' | 'event';
		content: string;
		thinkingContent: string;
		toolExecutions?: ToolExecution[];
		streaming: boolean;
		thinking: boolean;
		timestamp: string;
		source?: string;
		ephemeral?: boolean;
		userId?: string;
		/** True for messages created during this session (not loaded from DB). */
		live?: boolean;
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

	const PAGE_SIZE = 50;

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
		role: m.role,
		content: m.content,
		thinkingContent: m.reasoning ?? '',
		toolExecutions: m.tool_executions,
		streaming: false,
		thinking: false,
		timestamp: fmtTime(m.created_at),
		userId: m.user_id
	});

	/** Convert raw API messages to ChatMsg format. Tool executions are inline on assistant messages. */
	const toChatMessages = (rawMessages: Message[]): ChatMsg[] => rawMessages.map(toChat);

	let messages = $state<ChatMsg[]>([]);
	let loadingMore = $state(false);
	let allLoaded = $state(false);
	let sentinel: HTMLDivElement | undefined = $state();
	let assistantPhase = $state<AssistantPhase>('idle');
	const isBusy = $derived(assistantPhase !== 'idle');

	let messageQueue = $state<QueuedMessage[]>([]);
	let editingQueueId = $state<string | null>(null);
	let editingContent = $state('');

	let isAtBottom = $state(true);
	let messagesContainer: HTMLDivElement | undefined = $state();
	let title = $state('');
	let tags = $state<Tag[]>([]);
	let editingTitle = $state(false);
	let editTitleValue = $state('');
	let pendingConfirms = $state<ConfirmRequest[]>([]);
	let permissionMode = $state<PermissionMode>('policy_based');
	let deleteTarget = $state<string | null>(null);
	let showClearConfirm = $state(false);
	let settingsOpen = $state(false);
	let messageTags = $state<Record<string, Tag[]>>({});
	let skills = $state<SkillInfo[]>([]);

	const conversationId = $derived($page.params.id ?? '');
	const isGroup = $derived.by(() => {
		const storeConv = conversations.items.find((c) => c.id === data.conversation.id);
		return (storeConv?.kind ?? data.conversation.kind) === 'group';
	});
	let memberMap = $state<Record<string, string>>({});

	// Load collaborators for group conversations
	$effect(() => {
		if (isGroup) {
			const convId = data.conversation.id;
			untrack(() => {
				conversationService.listCollaborators(convId).then((collaborators) => {
					const map: Record<string, string> = {};
					for (const c of collaborators) map[c.user_id] = c.username;
					memberMap = map;
				});
			});
		} else {
			memberMap = {};
		}
	});

	// Fetch available skills when conversation changes
	$effect(() => {
		const id = conversationId;
		skillsService
			.list(id || undefined)
			.then((s) => {
				skills = s;
			})
			.catch(() => {
				skills = [];
			});
	});

	// Reset state when conversation changes
	$effect(() => {
		void conversationId;
		messages = toChatMessages(data.messages);
		allLoaded = data.messages.length < PAGE_SIZE;
		assistantPhase = 'idle';
		messageQueue = [];
		editingQueueId = null;
		isAtBottom = true;
		title = data.conversation.title || '';
		tags = data.conversation.tags ?? [];
		pendingConfirms = [];
		permissionMode = data.conversation.permission_mode ?? 'policy_based';
		memberMap = {};

		// Populate message tags from inline data.
		const tagMap: Record<string, Tag[]> = {};
		for (const m of data.messages) {
			if (m.tags && m.tags.length > 0) {
				tagMap[m.id] = m.tags;
			}
		}
		messageTags = tagMap;

		untrack(() => {
			conversations.markRead(data.conversation.id);
		});
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
		if (older.length < PAGE_SIZE) allLoaded = true;
		if (older.length > 0 && container) {
			const prevHeight = container.scrollHeight;
			messages = [...toChatMessages(older), ...messages];
			// Merge tags from older messages.
			for (const m of older) {
				if (m.tags && m.tags.length > 0) {
					messageTags[m.id] = m.tags;
				}
			}
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
			id: uuid(),
			role: 'user',
			content,
			thinkingContent: '',
			streaming: false,
			thinking: false,
			timestamp: now
		});
		messages.push({
			id: uuid(),
			role: 'assistant',
			content: '',
			thinkingContent: '',
			streaming: false,
			thinking: true,
			timestamp: now,
			live: true
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
			messageQueue.push({ id: uuid(), content });
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
			case 'chat.agent_typing': {
				// Show thinking indicator if no active assistant message.
				const last = messages[messages.length - 1];
				if (!last || last.role !== 'assistant' || (!last.thinking && !last.streaming)) {
					messages.push({
						id: uuid(),
						role: 'assistant',
						content: '',
						thinkingContent: '',
						streaming: false,
						thinking: true,
						timestamp: fmtTime(),
						live: true
					});
					assistantPhase = 'thinking';
				}
				break;
			}
			case 'chat.thinking': {
				let last = messages[messages.length - 1];
				if (last && last.role === 'assistant' && (last.thinking || last.streaming)) {
					last.thinkingContent += msg.content;
				} else {
					// No active assistant message — create a thinking placeholder
					// (happens for other group members who didn't send the message).
					messages.push({
						id: uuid(),
						role: 'assistant',
						content: '',
						thinkingContent: msg.content,
						streaming: false,
						thinking: true,
						timestamp: fmtTime(),
						live: true
					});
					assistantPhase = 'thinking';
				}
				break;
			}
			case 'chat.delta': {
				const last = messages[messages.length - 1];
				if (last && last.role === 'assistant' && (last.thinking || last.streaming)) {
					last.thinking = false;
					last.streaming = true;
					last.content += msg.content;
					assistantPhase = 'streaming';
				} else {
					messages.push({
						id: uuid(),
						role: 'assistant',
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
			case 'chat.tool_execution_update': {
				// Find the assistant message by message_id, or fall back to the last assistant.
				let target = messages.find((m) => m.id === msg.message_id && m.role === 'assistant');
				if (!target) {
					target = messages[messages.length - 1];
					if (!target || target.role !== 'assistant') break;
				}

				if (!target.toolExecutions) target.toolExecutions = [];

				// Upsert by execution id — replace with new object to trigger Svelte reactivity.
				const idx = target.toolExecutions.findIndex((te) => te.id === msg.id);
				if (idx >= 0) {
					target.toolExecutions = target.toolExecutions.map((te) =>
						te.id === msg.id
							? {
									...te,
									status: msg.status,
									output: msg.output ?? te.output,
									error: msg.error ?? te.error
								}
							: te
					);
				} else {
					target.toolExecutions = [
						...target.toolExecutions,
						{
							id: msg.id,
							tool_call_id: msg.tool_call_id,
							tool_name: msg.tool_name,
							input: msg.input ? JSON.parse(msg.input) : {},
							source: 'builtin',
							status: msg.status,
							output: msg.output,
							error: msg.error
						}
					];
				}

				target.streaming = true;
				target.thinking = false;
				messages = [...messages];
				break;
			}
			case 'chat.new_message': {
				// Own user messages were added optimistically — update the ID
				// to the real DB ID so tagging/deletion works.
				if (msg.role === 'user' && msg.user_id === auth.user?.id) {
					const ownMsg = [...messages].reverse().find((m) => m.role === 'user');
					if (ownMsg) ownMsg.id = msg.message_id;
					break;
				}

				// User messages from others — add directly, don't match with assistant.
				if (msg.role === 'user') {
					messages.push({
						id: msg.message_id,
						role: 'user',
						content: msg.content,
						thinkingContent: '',
						streaming: false,
						thinking: false,
						timestamp: fmtTime(),
						userId: msg.user_id
					});
					if (msg.user_id && msg.username) {
						memberMap[msg.user_id] = msg.username;
					}
					// Mark as read — user is actively viewing this conversation.
					untrack(() => conversations.markRead(conversationId));
					conversationService.markRead(conversationId);
					break;
				}

				// If an assistant message is actively streaming, update its
				// ID and source from the stored-message notification.
				const active = messages[messages.length - 1];
				if (active?.role === 'assistant' && (active.streaming || active.thinking)) {
					active.id = msg.message_id;
					if (msg.source && msg.source !== 'human') active.source = msg.source;
					break;
				}
				// Also check the last completed assistant message (done
				// already fired before new_message arrived).
				const prev = messages[messages.length - 1];
				if (prev?.role === 'assistant' && !prev.streaming && !prev.thinking) {
					if (msg.source && msg.source !== 'human') prev.source = msg.source;
					break;
				}
				// New message from scheduler or agent.
				const newMsg: ChatMsg = {
					id: msg.message_id,
					role: msg.role as ChatMsg['role'],
					content: msg.content,
					thinkingContent: '',
					streaming: false,
					thinking: false,
					timestamp: fmtTime(),
					source: msg.source,
					userId: msg.user_id
				};
				messages.push(newMsg);
				// Update memberMap if username is provided
				if (msg.user_id && msg.username && !memberMap[msg.user_id]) {
					memberMap[msg.user_id] = msg.username;
				}
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
				// Mark as read — user is actively viewing this conversation.
				untrack(() => conversations.markRead(conversationId));
				conversationService.markRead(conversationId);
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
			case 'chat.collaborator_added': {
				memberMap = { ...memberMap, [msg.user.id]: msg.user.username };
				break;
			}
			case 'chat.collaborator_removed': {
				// If the current user was removed (kicked), navigate away.
				if (msg.user_id === auth.user?.id) {
					conversations.remove(conversationId);
					goto(resolve('/'));
					return;
				}
				const updated = { ...memberMap };
				delete updated[msg.user_id];
				memberMap = updated;
				break;
			}
			case 'chat.role_changed': {
				break;
			}
			case 'chat.error': {
				const last = messages[messages.length - 1];
				if (last && (last.streaming || last.thinking)) {
					last.streaming = false;
					last.thinking = false;
					if (last.toolExecutions) {
						for (const te of last.toolExecutions) {
							if (te.status === 'pending' || te.status === 'running') {
								te.status = 'failed';
								te.error = msg.error;
							}
						}
					}
					last.content += `\n\nError: ${msg.error}`;
				} else {
					messages.push({
						id: uuid(),
						role: 'system',
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
		if (!tags.some((t) => t.id === tag.id)) {
			tags = [...tags, tag];
		}
		conversations.updateTags(data.conversation.id, tags);
	};

	const handleRemoveTag = async (tagId: string) => {
		tags = tags.filter((t) => t.id !== tagId);
		conversations.updateTags(data.conversation.id, tags);
		await tagService.removeFromConversation(data.conversation.id, tagId);
	};

	const confirmDelete = async () => {
		if (!deleteTarget) return;
		const id = deleteTarget;
		deleteTarget = null;
		await conversationService.deleteMessage(id);
		messages = messages.filter((m) => m.id !== id);
	};

	const confirmClear = async () => {
		showClearConfirm = false;
		await conversationService.clearMessages(conversationId);
		messages = [];
	};

	const handleSlashCommand = async (command: string) => {
		switch (command) {
			case '/help':
				messages.push({
					id: uuid(),
					role: 'system',
					content:
						'**Available commands:**\n- `/help` — Show available commands\n- `/info` — Show conversation info\n- `/clear` — Clear all messages\n- `/reload-skills` — Reload skills from disk\n\nType `/` to see available skills.',
					thinkingContent: '',
					streaming: false,
					thinking: false,
					timestamp: fmtTime(),
					ephemeral: true
				});
				break;
			case '/info':
				messages.push({
					id: uuid(),
					role: 'system',
					content: `**Conversation info:**\n- ID: \`${conversationId}\`\n- Title: ${title || 'Untitled'}\n- Messages: ${messages.length}`,
					thinkingContent: '',
					streaming: false,
					thinking: false,
					timestamp: fmtTime(),
					ephemeral: true
				});
				break;
			case '/clear':
				showClearConfirm = true;
				break;
			case '/reload-skills':
				try {
					const reloaded = await skillsService.reload(conversationId || undefined);
					skills = reloaded;
					messages.push({
						id: uuid(),
						role: 'system',
						content: `Skills reloaded. ${reloaded.length} skill(s) available.`,
						thinkingContent: '',
						streaming: false,
						thinking: false,
						timestamp: fmtTime(),
						ephemeral: true
					});
				} catch {
					messages.push({
						id: uuid(),
						role: 'system',
						content: 'Failed to reload skills.',
						thinkingContent: '',
						streaming: false,
						thinking: false,
						timestamp: fmtTime(),
						ephemeral: true
					});
				}
				break;
			default:
				// Skill command — send as regular message with /skill-name prefix
				if (skills.some((s) => `/${s.name}` === command)) {
					sendMessage(command);
				}
				break;
		}
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

	const handleArchive = async () => {
		const newArchived = !data.conversation.is_archived;
		await conversationService.archive(data.conversation.id, newArchived);
		if (newArchived) {
			conversations.archive(data.conversation.id);
		} else {
			conversations.unarchive(data.conversation.id);
		}
	};

	const handleDeleteConversation = async () => {
		await conversationService.delete(data.conversation.id);
		conversations.remove(data.conversation.id);
		goto(resolve('/'));
	};
</script>

<div class="flex h-full flex-col">
	<header
		class="flex flex-col justify-center gap-1.5 border-b border-zinc-200 px-4 py-2 dark:border-zinc-800"
	>
		<div class="flex items-center gap-2">
			{#if editingTitle}
				<input
					type="text"
					bind:value={editTitleValue}
					onkeydown={(e) => {
						if (e.key === 'Enter') saveTitle();
						if (e.key === 'Escape') editingTitle = false;
					}}
					onblur={saveTitle}
					class="min-w-0 flex-1 rounded border border-zinc-300 bg-transparent px-2 py-1 text-sm font-medium text-zinc-900 outline-none focus:border-zinc-500 dark:border-zinc-700 dark:text-zinc-100 dark:focus:border-zinc-500"
				/>
			{:else}
				<button
					onclick={startEditTitle}
					class="min-w-0 flex-1 truncate text-left text-sm font-medium text-zinc-900 hover:text-zinc-600 dark:text-zinc-100 dark:hover:text-zinc-400"
					title="Click to rename"
				>
					{title || 'New conversation'}
				</button>
			{/if}
			<button
				onclick={() => (settingsOpen = true)}
				class="shrink-0 rounded p-1 text-zinc-500 hover:bg-zinc-100 hover:text-zinc-700 dark:text-zinc-400 dark:hover:bg-zinc-800 dark:hover:text-zinc-200"
				title="Conversation settings"
			>
				<svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
					<path
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-width="2"
						d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"
					/>
					<path
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-width="2"
						d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
					/>
				</svg>
			</button>
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
					toolExecutions={msg.toolExecutions}
					streaming={msg.streaming}
					thinking={msg.thinking}
					live={msg.live}
					timestamp={msg.timestamp}
					source={msg.source}
					ephemeral={msg.ephemeral}
					messageId={msg.id}
					senderUsername={isGroup ? (memberMap[msg.userId ?? ''] ?? undefined) : undefined}
					tags={messageTags[msg.id] ?? []}
					onTagsChange={(newTags) => {
						messageTags[msg.id] = newTags;
					}}
					onDelete={msg.role === 'user' && !msg.ephemeral
						? () => {
								deleteTarget = msg.id;
							}
						: undefined}
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

	<ChatInput onsend={sendMessage} busy={isBusy} onSlashCommand={handleSlashCommand} {skills} />
	<StatusBar mode={permissionMode} onModeChange={handleModeChange} />
</div>

<ConversationSettings
	open={settingsOpen}
	conversation={data.conversation}
	{tags}
	{permissionMode}
	onClose={() => (settingsOpen = false)}
	onUpdateTitle={(t) => {
		title = t;
		conversations.updateTitle(conversationId, t);
		conversationService.updateTitle(conversationId, t);
	}}
	onUpdatePermissionMode={handleModeChange}
	onAddTag={handleAddTag}
	onRemoveTag={handleRemoveTag}
	onArchive={handleArchive}
	onClearHistory={confirmClear}
	onDelete={handleDeleteConversation}
/>

<ConfirmDialog
	open={!!deleteTarget}
	title="Delete message"
	message="Are you sure you want to delete this message? This cannot be undone."
	confirmText="Delete"
	destructive
	onConfirm={confirmDelete}
	onCancel={() => {
		deleteTarget = null;
	}}
/>

<ConfirmDialog
	open={showClearConfirm}
	title="Clear all messages"
	message="Are you sure you want to clear all messages in this conversation? This cannot be undone."
	confirmText="Clear"
	destructive
	onConfirm={confirmClear}
	onCancel={() => {
		showClearConfirm = false;
	}}
/>

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
