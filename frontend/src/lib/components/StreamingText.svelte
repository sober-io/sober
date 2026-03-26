<script lang="ts">
	import { renderMarkdown, highlighterReady } from '$lib/utils/markdown.svelte';

	interface Props {
		content: string;
		streaming?: boolean;
	}

	let { content, streaming = false }: Props = $props();

	const renderedContent = $derived(
		// Read highlighterReady.version to re-derive when shiki finishes loading
		content ? (highlighterReady.version, renderMarkdown(content)) : ''
	);
</script>

<div class="chat-prose prose prose-sm max-w-none inline">
	<!-- eslint-disable-next-line svelte/no-at-html-tags -- DOMPurify-sanitized in renderMarkdown -->
	{@html renderedContent}
</div>
{#if streaming}<span
		class="ml-0.5 inline-block h-4 w-0.5 animate-pulse bg-zinc-900 dark:bg-zinc-100"
	></span>{/if}
