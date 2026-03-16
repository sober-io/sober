<script lang="ts">
	import type { Conversation } from '$lib/types';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';

	interface Props {
		data: { conversations: Conversation[]; inbox: Conversation };
	}

	let { data }: Props = $props();
	let search = $state('');

	let unread = $derived(
		data.conversations
			.filter((c) => c.unread_count > 0)
			.sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime())
	);

	let recent = $derived(
		data.conversations
			.filter((c) => !c.is_archived && c.kind !== 'inbox')
			.filter((c) => !search || (c.title?.toLowerCase().includes(search.toLowerCase()) ?? false))
			.sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime())
			.slice(0, 20)
	);

	function formatRelativeTime(dateStr: string): string {
		const date = new Date(dateStr);
		const now = new Date();
		const diffMs = now.getTime() - date.getTime();
		const diffMins = Math.floor(diffMs / 60_000);
		const diffHours = Math.floor(diffMins / 60);
		const diffDays = Math.floor(diffHours / 24);

		if (diffMins < 1) return 'just now';
		if (diffMins < 60) return `${diffMins}m ago`;
		if (diffHours < 24) return `${diffHours}h ago`;
		if (diffDays < 7) return `${diffDays}d ago`;
		return date.toLocaleDateString();
	}

	async function createConversation() {
		const { conversationService } = await import('$lib/services/conversations');
		const conv = await conversationService.create();
		goto(resolve('/(app)/chat/[id]', { id: conv.id }));
	}
</script>

<div class="flex flex-1 flex-col overflow-y-auto p-6">
	<div class="mx-auto w-full max-w-2xl space-y-8">
		<!-- Header -->
		<div class="flex items-center justify-between">
			<h1 class="text-2xl font-semibold text-zinc-900 dark:text-zinc-100">Conversations</h1>
			<div class="flex items-center gap-3">
				<a
					href={resolve('/(app)/chat/[id]', { id: data.inbox.id })}
					class="flex items-center gap-2 rounded-lg px-3 py-1.5 text-sm font-medium text-zinc-600 transition-colors hover:bg-zinc-100 hover:text-zinc-900 dark:text-zinc-400 dark:hover:bg-zinc-800 dark:hover:text-zinc-100"
				>
					<svg
						class="size-4"
						xmlns="http://www.w3.org/2000/svg"
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						stroke-width="2"
						stroke-linecap="round"
						stroke-linejoin="round"
					>
						<polyline points="22 12 16 12 14 15 10 15 8 12 2 12"></polyline>
						<path
							d="M5.45 5.11L2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"
						></path>
					</svg>
					Inbox
					{#if data.inbox.unread_count > 0}
						<span
							class="flex size-5 items-center justify-center rounded-full bg-emerald-500 text-xs font-semibold text-white"
						>
							{data.inbox.unread_count}
						</span>
					{/if}
				</a>
				<button
					onclick={createConversation}
					class="flex items-center gap-2 rounded-lg bg-emerald-600 px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-emerald-700 dark:bg-emerald-500 dark:hover:bg-emerald-600"
				>
					<svg
						class="size-4"
						xmlns="http://www.w3.org/2000/svg"
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						stroke-width="2"
						stroke-linecap="round"
						stroke-linejoin="round"
					>
						<line x1="12" y1="5" x2="12" y2="19"></line>
						<line x1="5" y1="12" x2="19" y2="12"></line>
					</svg>
					New conversation
				</button>
			</div>
		</div>

		<!-- Search -->
		<div class="relative">
			<svg
				class="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-zinc-400"
				xmlns="http://www.w3.org/2000/svg"
				viewBox="0 0 24 24"
				fill="none"
				stroke="currentColor"
				stroke-width="2"
				stroke-linecap="round"
				stroke-linejoin="round"
			>
				<circle cx="11" cy="11" r="8"></circle>
				<line x1="21" y1="21" x2="16.65" y2="16.65"></line>
			</svg>
			<input
				type="search"
				placeholder="Search conversations…"
				bind:value={search}
				class="w-full rounded-lg border border-zinc-200 bg-white py-2 pl-9 pr-4 text-sm text-zinc-900 placeholder-zinc-400 transition-colors focus:border-emerald-500 focus:outline-none focus:ring-1 focus:ring-emerald-500 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:placeholder-zinc-500 dark:focus:border-emerald-400 dark:focus:ring-emerald-400"
			/>
		</div>

		<!-- Unread section -->
		{#if !search}
			<section>
				<h2
					class="mb-3 text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400"
				>
					Unread
				</h2>
				{#if unread.length === 0}
					<p class="text-sm text-zinc-400 dark:text-zinc-500">All caught up</p>
				{:else}
					<ul class="space-y-1">
						{#each unread as conv (conv.id)}
							<li>
								<a
									href={resolve('/(app)/chat/[id]', { id: conv.id })}
									class="flex items-center justify-between rounded-lg px-3 py-2.5 transition-colors hover:bg-zinc-100 dark:hover:bg-zinc-800"
								>
									<div class="min-w-0 flex-1">
										<p class="truncate text-sm font-medium text-zinc-900 dark:text-zinc-100">
											{conv.title ?? 'Untitled conversation'}
										</p>
										<p class="text-xs text-zinc-500 dark:text-zinc-400">
											{formatRelativeTime(conv.updated_at)}
										</p>
									</div>
									<span
										class="ml-3 flex min-w-[1.25rem] items-center justify-center rounded-full bg-emerald-500 px-1.5 py-0.5 text-xs font-semibold text-white"
									>
										{conv.unread_count}
									</span>
								</a>
							</li>
						{/each}
					</ul>
				{/if}
			</section>
		{/if}

		<!-- Recent conversations -->
		<section>
			<h2
				class="mb-3 text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400"
			>
				{search ? 'Results' : 'Recent'}
			</h2>
			{#if recent.length === 0}
				<p class="text-sm text-zinc-400 dark:text-zinc-500">
					{search ? 'No conversations match your search.' : 'No conversations yet.'}
				</p>
			{:else}
				<ul class="space-y-1">
					{#each recent as conv (conv.id)}
						<li>
							<a
								href={resolve('/(app)/chat/[id]', { id: conv.id })}
								class="flex items-center justify-between rounded-lg px-3 py-2.5 transition-colors hover:bg-zinc-100 dark:hover:bg-zinc-800"
							>
								<div class="min-w-0 flex-1">
									<p class="truncate text-sm font-medium text-zinc-900 dark:text-zinc-100">
										{conv.title ?? 'Untitled conversation'}
									</p>
									<div class="mt-0.5 flex items-center gap-2">
										<p class="text-xs text-zinc-500 dark:text-zinc-400">
											{formatRelativeTime(conv.updated_at)}
										</p>
										{#if conv.tags.length > 0}
											<div class="flex items-center gap-1">
												{#each conv.tags.slice(0, 3) as tag (tag.id)}
													<span
														class="rounded px-1 py-0.5 text-xs font-medium"
														style="background-color: color-mix(in srgb, {tag.color} 15%, transparent); color: {tag.color}; border: 1px solid color-mix(in srgb, {tag.color} 30%, transparent);"
													>
														{tag.name}
													</span>
												{/each}
											</div>
										{/if}
									</div>
								</div>
								{#if conv.unread_count > 0}
									<span
										class="ml-3 flex min-w-[1.25rem] items-center justify-center rounded-full bg-emerald-500 px-1.5 py-0.5 text-xs font-semibold text-white"
									>
										{conv.unread_count}
									</span>
								{/if}
							</a>
						</li>
					{/each}
				</ul>
			{/if}
		</section>
	</div>
</div>
