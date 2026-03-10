<script lang="ts">
	import type { Conversation } from '$lib/types';

	interface Props {
		conversations: Conversation[];
		activeId?: string;
		oncreate: () => void;
		onselect: (id: string) => void;
	}

	let { conversations, activeId, oncreate, onselect }: Props = $props();

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

<div class="flex flex-col">
	<button
		onclick={oncreate}
		class="mx-3 mt-3 mb-2 rounded-md border border-zinc-300 px-3 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-200 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
	>
		+ New chat
	</button>

	<nav class="flex-1 space-y-0.5 px-2">
		{#each conversations as conv (conv.id)}
			<button
				onclick={() => onselect(conv.id)}
				class={[
					'w-full rounded-md px-3 py-2 text-left text-sm transition-colors',
					activeId === conv.id
						? 'bg-zinc-200 text-zinc-900 dark:bg-zinc-800 dark:text-zinc-100'
						: 'text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800/50'
				]}
			>
				<div class="truncate font-medium">
					{conv.title || 'New conversation'}
				</div>
				<div class="mt-0.5 text-xs text-zinc-400 dark:text-zinc-500">
					{timeAgo(conv.updated_at)}
				</div>
			</button>
		{/each}

		{#if conversations.length === 0}
			<p class="px-3 py-4 text-center text-sm text-zinc-400 dark:text-zinc-500">
				No conversations yet
			</p>
		{/if}
	</nav>
</div>
