<script lang="ts">
	import { renderMarkdown, highlighterReady } from '$lib/utils/markdown.svelte';

	interface Props {
		content: string;
		streaming?: boolean;
	}

	let { content, streaming = false }: Props = $props();

	// eslint-disable-next-line @typescript-eslint/no-unused-vars -- read to trigger re-derive when shiki loads
	const _hlv = $derived(highlighterReady.version);
	const renderedContent = $derived(content ? renderMarkdown(content) : '');
</script>

<div class="chat-prose prose prose-sm max-w-none inline">
	<!-- eslint-disable-next-line svelte/no-at-html-tags -- DOMPurify-sanitized in renderMarkdown -->
	{@html renderedContent}
</div>
{#if streaming}<span
		class="ml-0.5 inline-block h-4 w-0.5 animate-pulse bg-zinc-900 dark:bg-zinc-100"
	></span>{/if}
