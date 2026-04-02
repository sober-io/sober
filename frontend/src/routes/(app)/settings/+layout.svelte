<script lang="ts">
	import type { Snippet } from 'svelte';
	import { page } from '$app/stores';
	import { resolve } from '$app/paths';
	import { isAdmin } from '$lib/guards';

	interface Props {
		children: Snippet;
	}

	let { children }: Props = $props();

	const activePath = $derived($page.url.pathname);
	const showAdmin = $derived(isAdmin());

	const tabClass = (path: string) =>
		activePath.startsWith(path)
			? 'border-b-2 border-zinc-900 px-4 py-2 text-sm font-medium text-zinc-900 dark:border-zinc-100 dark:text-zinc-100'
			: 'border-b-2 border-transparent px-4 py-2 text-sm font-medium text-zinc-500 hover:border-zinc-300 hover:text-zinc-700 dark:text-zinc-400 dark:hover:border-zinc-600 dark:hover:text-zinc-200';
</script>

<div class="h-full overflow-y-auto px-8 py-6">
	<h1 class="mb-6 text-xl font-semibold text-zinc-900 dark:text-zinc-100">Settings</h1>

	<div class="mb-6 flex gap-1 border-b border-zinc-200 dark:border-zinc-800">
		{#if showAdmin}
			<a
				href={resolve('/(app)/settings/evolution')}
				class={tabClass(resolve('/(app)/settings/evolution'))}
			>
				Evolution
			</a>
		{/if}
		<a
			href={resolve('/(app)/settings/plugins')}
			class={tabClass(resolve('/(app)/settings/plugins'))}
		>
			Plugins
		</a>
	</div>

	{@render children()}
</div>
