<script lang="ts">
	import type { Snippet } from 'svelte';
	import { auth } from '$lib/stores/auth.svelte';
	import { conversations } from '$lib/stores/conversations.svelte';
	import { conversationService } from '$lib/services/conversations';
	import { authService } from '$lib/services/auth';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/stores';

	interface Props {
		children: Snippet;
	}

	let { children }: Props = $props();

	let sidebarOpen = $state(false);

	// Per-conversation context menu state: stores the id of the open menu
	let openMenuId = $state<string | null>(null);

	const activeId = $derived($page.params.id ?? '');

	const visibleConversations = $derived(
		conversations.items.filter(
			(c) => c.kind !== 'inbox' && (conversations.showArchived || !c.is_archived)
		)
	);

	$effect(() => {
		loadConversations();
	});

	const loadConversations = async () => {
		try {
			conversations.set(await conversationService.list());
		} catch {
			conversations.set([]);
		}
		// Load inbox separately
		try {
			const inbox = await conversationService.getInbox();
			conversations.setInbox(inbox);
		} catch {
			// Inbox may not exist yet — ignore
		}
	};

	const createConversation = async () => {
		const conv = await conversationService.create();
		conversations.prepend(conv);
		goto(resolve('/(app)/chat/[id]', { id: conv.id }));
	};

	const selectConversation = (id: string) => {
		sidebarOpen = false;
		openMenuId = null;
		goto(resolve('/(app)/chat/[id]', { id }));
	};

	const handleLogout = async () => {
		await authService.logout();
		auth.setUser(null);
		goto(resolve('/login'));
	};

	const handleArchive = async (id: string, isArchived: boolean) => {
		openMenuId = null;
		try {
			await conversationService.archive(id, !isArchived);
			if (!isArchived) {
				conversations.archive(id);
				// If we archived the active conversation, navigate away
				if (activeId === id) {
					goto(resolve('/(app)'));
				}
			} else {
				conversations.unarchive(id);
			}
		} catch {
			// Silently ignore errors for now
		}
	};

	const handleDelete = async (id: string) => {
		openMenuId = null;
		const confirmed = confirm('Delete this conversation? This cannot be undone.');
		if (!confirmed) return;
		try {
			await conversationService.delete(id);
			conversations.remove(id);
			if (activeId === id) {
				goto(resolve('/(app)'));
			}
		} catch {
			// Silently ignore errors for now
		}
	};

	const timeAgo = (dateStr: string): string => {
		const seconds = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
		if (seconds < 60) return 'just now';
		const minutes = Math.floor(seconds / 60);
		if (minutes < 60) return `${minutes}m ago`;
		const hours = Math.floor(minutes / 60);
		if (hours < 24) return `${hours}h ago`;
		const days = Math.floor(hours / 24);
		return `${days}d ago`;
	};
</script>

<!-- Close context menu on outside click -->
<svelte:window onclick={() => (openMenuId = null)} />

<div class="flex h-screen bg-white dark:bg-zinc-950">
	<!-- Mobile sidebar toggle -->
	<button
		class="fixed top-3 left-3 z-50 rounded-md p-2 text-zinc-600 hover:bg-zinc-100 md:hidden dark:text-zinc-400 dark:hover:bg-zinc-800"
		onclick={() => (sidebarOpen = !sidebarOpen)}
		aria-label="Toggle sidebar"
	>
		<svg class="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
			{#if sidebarOpen}
				<path
					stroke-linecap="round"
					stroke-linejoin="round"
					stroke-width="2"
					d="M6 18L18 6M6 6l12 12"
				/>
			{:else}
				<path
					stroke-linecap="round"
					stroke-linejoin="round"
					stroke-width="2"
					d="M4 6h16M4 12h16M4 18h16"
				/>
			{/if}
		</svg>
	</button>

	<!-- Sidebar backdrop (mobile) -->
	{#if sidebarOpen}
		<button
			class="fixed inset-0 z-30 bg-black/50 md:hidden"
			onclick={() => (sidebarOpen = false)}
			aria-label="Close sidebar"
		></button>
	{/if}

	<!-- Sidebar -->
	<aside
		class={[
			'fixed inset-y-0 left-0 z-40 flex w-72 flex-col border-r border-zinc-200 bg-zinc-50 transition-transform md:static md:translate-x-0 dark:border-zinc-800 dark:bg-zinc-900',
			sidebarOpen ? 'translate-x-0' : '-translate-x-full'
		]}
	>
		<!-- Header -->
		<div
			class="flex h-14 items-center justify-between border-b border-zinc-200 px-4 dark:border-zinc-800"
		>
			<span class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">Sõber</span>
			<button
				onclick={handleLogout}
				class="rounded-md px-2 py-1 text-xs text-zinc-500 hover:bg-zinc-200 hover:text-zinc-700 dark:text-zinc-400 dark:hover:bg-zinc-800 dark:hover:text-zinc-200"
			>
				Sign out
			</button>
		</div>

		<!-- Scrollable list area -->
		<div class="flex flex-1 flex-col overflow-hidden">
			{#if conversations.loading}
				<div class="p-4 text-sm text-zinc-500 dark:text-zinc-400">Loading...</div>
			{:else}
				<!-- New chat button -->
				<button
					onclick={createConversation}
					class="mx-3 mt-3 mb-2 rounded-md border border-zinc-300 px-3 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-200 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
				>
					+ New chat
				</button>

				<!-- Inbox — pinned above conversation list, never scrolls -->
				{#if conversations.inbox}
					{@const inbox = conversations.inbox}
					<div class="px-2 pb-1">
						<button
							onclick={() => selectConversation(inbox.id)}
							class={[
								'group relative flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm transition-colors',
								activeId === inbox.id
									? 'bg-zinc-200 text-zinc-900 dark:bg-zinc-800 dark:text-zinc-100'
									: 'text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800/50'
							]}
						>
							<!-- Inbox icon -->
							<svg
								class="h-4 w-4 shrink-0 text-emerald-500"
								fill="none"
								stroke="currentColor"
								viewBox="0 0 24 24"
								aria-hidden="true"
							>
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M20 13V6a2 2 0 00-2-2H6a2 2 0 00-2 2v7m16 0v5a2 2 0 01-2 2H6a2 2 0 01-2-2v-5m16 0h-2.586a1 1 0 00-.707.293l-2.414 2.414a1 1 0 01-.707.293h-3.172a1 1 0 01-.707-.293l-2.414-2.414A1 1 0 006.586 13H4"
								/>
							</svg>
							<span class="truncate font-medium">Inbox</span>
							{#if inbox.unread_count > 0}
								<span
									class="ml-auto shrink-0 rounded-full bg-emerald-500 px-1.5 py-0.5 text-xs font-semibold text-white"
								>
									{inbox.unread_count > 99 ? '99+' : inbox.unread_count}
								</span>
							{/if}
						</button>
					</div>

					<!-- Divider between inbox and conversations -->
					<div class="mx-3 mb-1 border-t border-zinc-200 dark:border-zinc-800"></div>
				{/if}

				<!-- Conversation list (scrollable) -->
				<nav class="flex-1 space-y-0.5 overflow-y-auto px-2 pb-2">
					{#each visibleConversations as conv (conv.id)}
						<div class="group relative">
							<button
								onclick={() => selectConversation(conv.id)}
								class={[
									'w-full rounded-md px-3 py-2 text-left text-sm transition-colors',
									activeId === conv.id
										? 'bg-zinc-200 text-zinc-900 dark:bg-zinc-800 dark:text-zinc-100'
										: 'text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800/50'
								]}
							>
								<!-- Title row: title + unread badge -->
								<div class="flex items-center gap-1.5">
									<span class="min-w-0 flex-1 truncate font-medium">
										{conv.title || 'New conversation'}
									</span>
									{#if conv.unread_count > 0}
										<span
											class="shrink-0 rounded-full bg-emerald-500 px-1.5 py-0.5 text-xs font-semibold text-white"
										>
											{conv.unread_count > 99 ? '99+' : conv.unread_count}
										</span>
									{/if}
								</div>

								<!-- Bottom row: time + tag pills -->
								<div class="mt-0.5 flex items-center gap-1">
									<span class="text-xs text-zinc-400 dark:text-zinc-500">
										{timeAgo(conv.updated_at)}
									</span>
									{#if conv.tags.length > 0}
										<span class="ml-1 flex items-center gap-1">
											{#each conv.tags.slice(0, 5) as tag (tag.id)}
												<span
													class="h-2 w-2 rounded-full"
													style="background-color: {tag.color}"
													title={tag.name}
												></span>
											{/each}
										</span>
									{/if}
								</div>
							</button>

							<!-- Context menu trigger — visible on group hover -->
							<div class="absolute top-1.5 right-1.5 opacity-0 group-hover:opacity-100">
								<button
									onclick={(e) => {
										e.stopPropagation();
										openMenuId = openMenuId === conv.id ? null : conv.id;
									}}
									class="rounded p-0.5 text-zinc-400 hover:bg-zinc-200 hover:text-zinc-700 dark:text-zinc-500 dark:hover:bg-zinc-700 dark:hover:text-zinc-200"
									aria-label="Conversation options"
								>
									<svg class="h-4 w-4" fill="currentColor" viewBox="0 0 20 20" aria-hidden="true">
										<path
											d="M6 10a2 2 0 11-4 0 2 2 0 014 0zM12 10a2 2 0 11-4 0 2 2 0 014 0zM16 12a2 2 0 100-4 2 2 0 000 4z"
										/>
									</svg>
								</button>

								{#if openMenuId === conv.id}
									<div
										role="menu"
										tabindex="-1"
										class="absolute right-0 top-6 z-50 min-w-[10rem] rounded-md border border-zinc-200 bg-white py-1 shadow-lg dark:border-zinc-700 dark:bg-zinc-800"
										onclick={(e) => e.stopPropagation()}
										onkeydown={(e) => e.stopPropagation()}
									>
										<button
											onclick={() => handleArchive(conv.id, conv.is_archived)}
											class="flex w-full items-center gap-2 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-zinc-700"
										>
											{#if conv.is_archived}
												<svg
													class="h-4 w-4"
													fill="none"
													stroke="currentColor"
													viewBox="0 0 24 24"
													aria-hidden="true"
												>
													<path
														stroke-linecap="round"
														stroke-linejoin="round"
														stroke-width="2"
														d="M3 10h18M3 10l4-4m-4 4l4 4M5 10h14v9a2 2 0 01-2 2H7a2 2 0 01-2-2v-9z"
													/>
												</svg>
												Unarchive
											{:else}
												<svg
													class="h-4 w-4"
													fill="none"
													stroke="currentColor"
													viewBox="0 0 24 24"
													aria-hidden="true"
												>
													<path
														stroke-linecap="round"
														stroke-linejoin="round"
														stroke-width="2"
														d="M5 8h14M5 8a2 2 0 110-4h14a2 2 0 110 4M5 8v10a2 2 0 002 2h10a2 2 0 002-2V8m-9 4h4"
													/>
												</svg>
												Archive
											{/if}
										</button>
										<button
											onclick={() => handleDelete(conv.id)}
											class="flex w-full items-center gap-2 px-3 py-1.5 text-sm text-red-600 hover:bg-zinc-100 dark:text-red-400 dark:hover:bg-zinc-700"
										>
											<svg
												class="h-4 w-4"
												fill="none"
												stroke="currentColor"
												viewBox="0 0 24 24"
												aria-hidden="true"
											>
												<path
													stroke-linecap="round"
													stroke-linejoin="round"
													stroke-width="2"
													d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
												/>
											</svg>
											Delete
										</button>
									</div>
								{/if}
							</div>
						</div>
					{/each}

					{#if visibleConversations.length === 0}
						<p class="px-3 py-4 text-center text-sm text-zinc-400 dark:text-zinc-500">
							No conversations yet
						</p>
					{/if}
				</nav>
			{/if}
		</div>

		<!-- Footer -->
		<div class="border-t border-zinc-200 dark:border-zinc-800">
			<!-- Archive toggle -->
			<div class="px-3 pt-2">
				<button
					onclick={() => conversations.setShowArchived(!conversations.showArchived)}
					class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-xs text-zinc-500 hover:bg-zinc-200 dark:text-zinc-400 dark:hover:bg-zinc-800"
				>
					<svg
						class="h-3.5 w-3.5"
						fill="none"
						stroke="currentColor"
						viewBox="0 0 24 24"
						aria-hidden="true"
					>
						<path
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-width="2"
							d="M5 8h14M5 8a2 2 0 110-4h14a2 2 0 110 4M5 8v10a2 2 0 002 2h10a2 2 0 002-2V8m-9 4h4"
						/>
					</svg>
					{conversations.showArchived ? 'Hide archived' : 'Show archived'}
				</button>
			</div>

			<div class="p-3">
				<a
					href={resolve('/settings/mcp')}
					class="block rounded-md px-3 py-2 text-sm text-zinc-600 hover:bg-zinc-200 dark:text-zinc-400 dark:hover:bg-zinc-800"
				>
					MCP Settings
				</a>
			</div>
		</div>
	</aside>

	<!-- Main content -->
	<main class="flex flex-1 flex-col overflow-hidden">
		{@render children()}
	</main>
</div>
