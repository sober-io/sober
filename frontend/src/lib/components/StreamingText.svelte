<script lang="ts">
	import { renderMarkdown } from '$lib/utils/markdown';

	interface Props {
		content: string;
		streaming?: boolean;
	}

	let { content, streaming = false }: Props = $props();

	const renderedContent = $derived(content ? renderMarkdown(content) : '');
</script>

<div class="chat-prose prose prose-sm max-w-none inline">
	<!-- eslint-disable-next-line svelte/no-at-html-tags -- DOMPurify-sanitized in renderMarkdown -->
	{@html renderedContent}
</div>
{#if streaming}<span
		class="ml-0.5 inline-block h-4 w-0.5 animate-pulse bg-zinc-900 dark:bg-zinc-100"
	></span>{/if}
