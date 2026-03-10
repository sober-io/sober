<script lang="ts">
	import { renderMarkdown } from '$lib/utils/markdown';

	interface Props {
		content: string;
		streaming?: boolean;
	}

	let { content, streaming = false }: Props = $props();

	const renderedContent = $derived(content ? renderMarkdown(content) : '');
</script>

<div class="prose prose-sm dark:prose-invert max-w-none inline">
	{@html renderedContent}
</div>
{#if streaming}<span
		class="ml-0.5 inline-block h-4 w-0.5 animate-pulse bg-zinc-900 dark:bg-zinc-100"
	></span>{/if}
