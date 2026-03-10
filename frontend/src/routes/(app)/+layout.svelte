<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { Conversation } from '$lib/types';
	import { auth } from '$lib/stores/auth.svelte';
	import { conversationService } from '$lib/services/conversations';
	import { authService } from '$lib/services/auth';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/stores';
	import ConversationList from '$lib/components/ConversationList.svelte';

	interface Props {
		children: Snippet;
	}

	let { children }: Props = $props();

	let conversations = $state<Conversation[]>([]);
	let sidebarOpen = $state(false);
	let loadingConversations = $state(true);

	const activeId = $derived($page.params.id ?? '');

	$effect(() => {
		loadConversations();
	});

	const loadConversations = async () => {
		try {
			conversations = await conversationService.list();
		} catch {
			conversations = [];
		} finally {
			loadingConversations = false;
		}
	};

	const createConversation = async () => {
		const conv = await conversationService.create();
		conversations = [conv, ...conversations];
		goto(resolve('/(app)/chat/[id]', { id: conv.id }));
	};

	const selectConversation = (id: string) => {
		sidebarOpen = false;
		goto(resolve('/(app)/chat/[id]', { id }));
	};

	const handleLogout = async () => {
		await authService.logout();
		auth.setUser(null);
		goto(resolve('/login'));
	};
</script>

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

		<div class="flex-1 overflow-y-auto">
			{#if loadingConversations}
				<div class="p-4 text-sm text-zinc-500 dark:text-zinc-400">Loading...</div>
			{:else}
				<ConversationList
					{conversations}
					{activeId}
					oncreate={createConversation}
					onselect={selectConversation}
				/>
			{/if}
		</div>

		<div class="border-t border-zinc-200 p-3 dark:border-zinc-800">
			<a
				href={resolve('/settings/mcp')}
				class="block rounded-md px-3 py-2 text-sm text-zinc-600 hover:bg-zinc-200 dark:text-zinc-400 dark:hover:bg-zinc-800"
			>
				MCP Settings
			</a>
		</div>
	</aside>

	<!-- Main content -->
	<main class="flex flex-1 flex-col overflow-hidden">
		{@render children()}
	</main>
</div>
