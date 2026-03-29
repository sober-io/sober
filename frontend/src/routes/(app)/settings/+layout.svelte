<script lang="ts">
	import type { Snippet } from 'svelte';
	import { page } from '$app/stores';
	import { resolve } from '$app/paths';

	interface Props {
		children: Snippet;
	}

	let { children }: Props = $props();

	const tabs = [
		{ label: 'Evolution', href: resolve('/(app)/settings/evolution') },
		{ label: 'Plugins', href: resolve('/(app)/settings/plugins') }
	];

	const activePath = $derived($page.url.pathname);
</script>

<div class="mx-auto max-w-3xl p-6">
	<h1 class="mb-6 text-xl font-semibold text-zinc-900 dark:text-zinc-100">Settings</h1>

	<!-- Tab navigation -->
	<div class="mb-6 flex gap-1 border-b border-zinc-200 dark:border-zinc-800">
		{#each tabs as tab (tab.href)}
			<a
				href={tab.href}
				class={[
					'border-b-2 px-4 py-2 text-sm font-medium transition-colors',
					activePath.startsWith(tab.href)
						? 'border-zinc-900 text-zinc-900 dark:border-zinc-100 dark:text-zinc-100'
						: 'border-transparent text-zinc-500 hover:border-zinc-300 hover:text-zinc-700 dark:text-zinc-400 dark:hover:border-zinc-600 dark:hover:text-zinc-200'
				]}
			>
				{tab.label}
			</a>
		{/each}
	</div>

	{@render children()}
</div>
