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
	let outputExpanded = $state(false);

	const displayOutput = $derived(error ?? output);
	const showError = $derived(isError || !!error);

	const OUTPUT_LIMIT = 2000;
	const isOutputTruncated = $derived(
		displayOutput !== undefined && displayOutput.length > OUTPUT_LIMIT
	);
	const visibleOutput = $derived.by(() => {
		if (displayOutput === undefined) return undefined;
		if (outputExpanded || displayOutput.length <= OUTPUT_LIMIT) return displayOutput;
		return displayOutput.slice(0, OUTPUT_LIMIT);
	});

	/** Format a JSON value into colored HTML spans. */
	function formatJson(value: unknown, indent = 0): string {
		const pad = '  '.repeat(indent);
		const padInner = '  '.repeat(indent + 1);

		if (value === null) return '<span class="text-zinc-400">null</span>';
		if (value === undefined) return '<span class="text-zinc-400">undefined</span>';

		if (typeof value === 'string') {
			const escaped = value
				.replace(/&/g, '&amp;')
				.replace(/</g, '&lt;')
				.replace(/>/g, '&gt;')
				.replace(/"/g, '&quot;');
			if (value.length > 200) {
				const short = escaped.slice(0, 200);
				return `<span class="text-emerald-600 dark:text-emerald-400">"${short}…"</span>`;
			}
			return `<span class="text-emerald-600 dark:text-emerald-400">"${escaped}"</span>`;
		}

		if (typeof value === 'number')
			return `<span class="text-amber-600 dark:text-amber-400">${value}</span>`;
		if (typeof value === 'boolean')
			return `<span class="text-violet-600 dark:text-violet-400">${value}</span>`;

		if (Array.isArray(value)) {
			if (value.length === 0) return '[]';
			const items = value.map((v) => `${padInner}${formatJson(v, indent + 1)}`).join(',\n');
			return `[\n${items}\n${pad}]`;
		}

		if (typeof value === 'object') {
			const entries = Object.entries(value as Record<string, unknown>);
			if (entries.length === 0) return '{}';
			const lines = entries
				.map(
					([k, v]) =>
						`${padInner}<span class="text-sky-600 dark:text-sky-400">"${k}"</span>: ${formatJson(v, indent + 1)}`
				)
				.join(',\n');
			return `{\n${lines}\n${pad}}`;
		}

		return String(value);
	}

	const formattedInput = $derived(formatJson(input));
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
				class="h-3 w-3 text-emerald-500"
				fill="none"
				viewBox="0 0 24 24"
				stroke="currentColor"
				stroke-width="2"
			>
				<path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
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
			<!-- eslint-disable svelte/no-at-html-tags -- manually HTML-escaped in formatJson -->
			<pre
				class="max-h-60 max-w-full overflow-auto rounded bg-zinc-100 p-2 font-mono text-xs text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300">{@html formattedInput}</pre>
			<!-- eslint-enable svelte/no-at-html-tags -->

			{#if visibleOutput !== undefined}
				<div class="mt-2 mb-1 text-xs font-medium text-zinc-500 dark:text-zinc-400">Output</div>
				<pre
					class={[
						'max-h-80 max-w-full overflow-auto rounded bg-zinc-100 p-2 text-xs dark:bg-zinc-800',
						showError ? 'text-red-600 dark:text-red-400' : 'text-zinc-700 dark:text-zinc-300'
					]}>{visibleOutput}</pre>
				{#if isOutputTruncated && !outputExpanded}
					<button
						onclick={() => (outputExpanded = true)}
						class="mt-1 text-xs text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300"
					>
						Show full output ({displayOutput?.length.toLocaleString()} chars)
					</button>
				{/if}
			{/if}
		</div>
	{/if}
</div>
