<script lang="ts">
	import { highlighterReady } from '$lib/utils/markdown.svelte';
	import { getHighlighter } from '$lib/utils/markdown.svelte';

	interface Props {
		toolName: string;
		input: unknown;
		output?: string;
		error?: string;
		loading?: boolean;
		isError?: boolean;
		durationMs?: number;
	}

	let {
		toolName,
		input,
		output,
		error,
		loading = false,
		isError = false,
		durationMs
	}: Props = $props();
	let expanded = $state(false);
	let outputExpanded = $state(false);

	const displayOutput = $derived(error ?? output);
	const showError = $derived(isError || !!error);

	const durationLabel = $derived.by(() => {
		if (durationMs === undefined) return null;
		if (durationMs < 1000) return `${durationMs}ms`;
		return `${(durationMs / 1000).toFixed(1)}s`;
	});

	const OUTPUT_LIMIT = 2000;
	const isOutputTruncated = $derived(
		displayOutput !== undefined && displayOutput.length > OUTPUT_LIMIT
	);
	const visibleOutput = $derived.by(() => {
		if (displayOutput === undefined) return undefined;
		if (outputExpanded || displayOutput.length <= OUTPUT_LIMIT) return displayOutput;
		return displayOutput.slice(0, OUTPUT_LIMIT);
	});

	/** Try to pretty-print a string as JSON. Returns null if not valid JSON. */
	function tryFormatJson(str: string): string | null {
		const trimmed = str.trim();
		if (!(trimmed.startsWith('{') || trimmed.startsWith('['))) return null;
		try {
			return JSON.stringify(JSON.parse(trimmed), null, 2);
		} catch {
			return null;
		}
	}

	/** Highlight code with shiki if available, otherwise escape for <pre>. */
	function highlightCode(code: string, lang: string): string {
		const hl = getHighlighter();
		if (hl) {
			return hl.codeToHtml(code, { lang, theme: 'github-dark' });
		}
		return escapeHtml(code);
	}

	function escapeHtml(s: string): string {
		return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
	}

	const formattedInput = $derived.by(() => {
		const _v = highlighterReady.version; // re-derive when shiki loads
		void _v;
		const json = JSON.stringify(input, null, 2);
		return highlightCode(json, 'json');
	});

	/** Try to detect and highlight output content. */
	const formattedOutput = $derived.by(() => {
		if (visibleOutput === undefined) return undefined;
		const _v = highlighterReady.version; // re-derive when shiki loads
		void _v;
		const asJson = tryFormatJson(visibleOutput);
		if (asJson) return { html: highlightCode(asJson, 'json'), isHighlighted: true };
		return { html: escapeHtml(visibleOutput), isHighlighted: false };
	});
</script>

<div
	class="my-2 min-w-0 overflow-hidden rounded-md border border-zinc-200 text-sm dark:border-zinc-700"
>
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
		{#if durationLabel}
			<span class="text-[10px] text-zinc-400 dark:text-zinc-500">{durationLabel}</span>
		{/if}
		<svg
			class={['ml-auto h-3 w-3 shrink-0 transition-transform', expanded && 'rotate-90']}
			fill="currentColor"
			viewBox="0 0 20 20"
		>
			<path d="M6 4l8 6-8 6V4z" />
		</svg>
	</button>

	{#if expanded}
		<div class="border-t border-zinc-200 px-3 py-2 dark:border-zinc-700">
			<div class="mb-1 text-xs font-medium text-zinc-500 dark:text-zinc-400">Input</div>
			<!-- eslint-disable svelte/no-at-html-tags -- shiki-highlighted or manually escaped -->
			<div class="tool-code max-h-60 overflow-auto whitespace-pre-wrap break-words rounded text-xs">
				{@html formattedInput}
			</div>

			{#if formattedOutput !== undefined}
				<div class="mt-2 mb-1 text-xs font-medium text-zinc-500 dark:text-zinc-400">Output</div>
				{#if formattedOutput.isHighlighted}
					<div
						class="tool-code max-h-80 overflow-auto whitespace-pre-wrap break-words rounded text-xs"
					>
						{@html formattedOutput.html}
					</div>
				{:else}
					<pre
						class={[
							'max-h-80 overflow-auto whitespace-pre-wrap break-words rounded bg-zinc-100 p-2 text-xs dark:bg-zinc-800',
							showError ? 'text-red-600 dark:text-red-400' : 'text-zinc-700 dark:text-zinc-300'
						]}>{@html formattedOutput.html}</pre>
				{/if}
				{#if isOutputTruncated && !outputExpanded}
					<button
						onclick={() => (outputExpanded = true)}
						class="mt-1 text-xs text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300"
					>
						Show full output ({displayOutput?.length.toLocaleString()} chars)
					</button>
				{/if}
			{/if}
			<!-- eslint-enable svelte/no-at-html-tags -->
		</div>
	{/if}
</div>
