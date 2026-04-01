<script lang="ts">
	import type { ContentBlock, ConversationAttachment } from '$lib/types';
	import { renderMarkdown, highlighterReady } from '$lib/utils/markdown.svelte';
	import ImageBlock from './ImageBlock.svelte';
	import FileBlock from './FileBlock.svelte';

	interface Props {
		blocks: ContentBlock[];
		attachments?: Record<string, ConversationAttachment>;
		streaming?: boolean;
	}

	let { blocks, attachments, streaming = false }: Props = $props();
</script>

{#each blocks as block (block.type === 'text' ? `text-${blocks.indexOf(block)}` : block.type === 'image' || block.type === 'file' || block.type === 'audio' || block.type === 'video' ? block.conversation_attachment_id : blocks.indexOf(block))}
	{#if block.type === 'text'}
		{@const rendered = block.text ? (highlighterReady.version, renderMarkdown(block.text)) : ''}
		<!-- eslint-disable-next-line svelte/no-at-html-tags -- DOMPurify-sanitized in renderMarkdown -->
		<div class="chat-prose prose prose-sm max-w-none">{@html rendered}</div>
	{:else if block.type === 'image'}
		<ImageBlock attachmentId={block.conversation_attachment_id} alt={block.alt} />
	{:else if block.type === 'file'}
		<FileBlock
			attachmentId={block.conversation_attachment_id}
			attachment={attachments?.[block.conversation_attachment_id]}
		/>
	{:else if block.type === 'audio'}
		<audio
			controls
			src={`/api/v1/attachments/${block.conversation_attachment_id}/content`}
			class="my-1 max-w-full"
		>
			<track kind="captions" />
		</audio>
	{:else if block.type === 'video'}
		<!-- svelte-ignore a11y_media_has_caption -->
		<video
			controls
			src={`/api/v1/attachments/${block.conversation_attachment_id}/content`}
			class="my-1 max-h-96 max-w-full rounded-md"
		></video>
	{/if}
{/each}
{#if streaming}<span
		class="ml-0.5 inline-block h-4 w-0.5 animate-pulse bg-zinc-900 dark:bg-zinc-100"
	></span>{/if}
