<script lang="ts">
	interface Props {
		toolName: string;
		input: unknown;
		output?: string;
		error?: string;
		loading?: boolean;
		isError?: boolean;
	}

	let { toolName, input, output, error, loading = false, isError = false }: Props = $props();
	let expanded = $state(false);

	const displayOutput = $derived(error ?? output);
	const showError = $derived(isError || !!error);
</script>

<div class="my-2 rounded-md border border-zinc-200 text-sm dark:border-zinc-700">
	<button
		onclick={() => (expanded = !expanded)}
		class="flex w-full items-center gap-2 px-3 py-2 text-left text-zinc-600 hover:bg-zinc-50 dark:text-zinc-400 dark:hover:bg-zinc-800/50"
	>
		{#if loading}
			<span
				class="inline-block h-3 w-3 animate-spin rounded-full border-2 border-zinc-400 border-t-transparent"
			></span>
		{:else if showError}
			<svg
				class="h-3 w-3 text-red-500"
				fill="none"
				viewBox="0 0 24 24"
				stroke="currentColor"
				stroke-width="2"
			>
				<path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
			</svg>
		{:else}
			<svg
				class="h-3 w-3 transition-transform {expanded ? 'rotate-90' : ''}"
				fill="currentColor"
				viewBox="0 0 20 20"
			>
				<path d="M6 4l8 6-8 6V4z" />
			</svg>
		{/if}
		<span class="font-mono text-xs">{toolName}</span>
		{#if showError}
			<span class="text-xs text-red-500">failed</span>
		{/if}
	</button>

	{#if expanded}
		<div class="border-t border-zinc-200 px-3 py-2 dark:border-zinc-700">
			<div class="mb-1 text-xs font-medium text-zinc-500 dark:text-zinc-400">Input</div>
			<pre
				class="overflow-x-auto rounded bg-zinc-100 p-2 text-xs text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300">{JSON.stringify(
					input,
					null,
					2
				)}</pre>

			{#if displayOutput !== undefined}
				<div class="mt-2 mb-1 text-xs font-medium text-zinc-500 dark:text-zinc-400">Output</div>
				<pre
					class={[
						'overflow-x-auto rounded bg-zinc-100 p-2 text-xs dark:bg-zinc-800',
						showError ? 'text-red-600 dark:text-red-400' : 'text-zinc-700 dark:text-zinc-300'
					]}>{displayOutput}</pre>
			{/if}
		</div>
	{/if}
</div>
