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

	// Group blocks by type: text first, then images together, then files, audio, video.
	const groupedBlocks = $derived.by(() => {
		const text: ContentBlock[] = [];
		const images: ContentBlock[] = [];
		const files: ContentBlock[] = [];
		const media: ContentBlock[] = []; // audio + video

		for (const block of blocks) {
			switch (block.type) {
				case 'text':
					text.push(block);
					break;
				case 'image':
					images.push(block);
					break;
				case 'file':
					files.push(block);
					break;
				case 'audio':
				case 'video':
					media.push(block);
					break;
			}
		}

		return { text, images, files, media };
	});
</script>

<!-- Text blocks -->
{#each groupedBlocks.text as block, i (i)}
	{@const rendered =
		block.type === 'text' && block.text
			? (highlighterReady.version, renderMarkdown(block.text))
			: ''}
	<!-- eslint-disable-next-line svelte/no-at-html-tags -- DOMPurify-sanitized in renderMarkdown -->
	<div class="chat-prose prose prose-sm max-w-none">{@html rendered}</div>
{/each}

<!-- Images grouped with gap -->
{#if groupedBlocks.images.length > 0}
	<div class="mt-2 flex flex-wrap gap-2">
		{#each groupedBlocks.images as block (block.type === 'image' ? block.conversation_attachment_id : '')}
			{#if block.type === 'image'}
				<ImageBlock attachmentId={block.conversation_attachment_id} alt={block.alt} />
			{/if}
		{/each}
	</div>
{/if}

<!-- Files grouped -->
{#if groupedBlocks.files.length > 0}
	<div class="mt-2 flex flex-col gap-1.5">
		{#each groupedBlocks.files as block (block.type === 'file' ? block.conversation_attachment_id : '')}
			{#if block.type === 'file'}
				<FileBlock
					attachmentId={block.conversation_attachment_id}
					attachment={attachments?.[block.conversation_attachment_id]}
				/>
			{/if}
		{/each}
	</div>
{/if}

<!-- Audio/Video grouped -->
{#if groupedBlocks.media.length > 0}
	<div class="mt-2 flex flex-col gap-2">
		{#each groupedBlocks.media as block (block.type === 'audio' || block.type === 'video' ? block.conversation_attachment_id : '')}
			{#if block.type === 'audio'}
				<audio
					controls
					src={`/api/v1/attachments/${block.conversation_attachment_id}/content`}
					class="max-w-full"
				>
					<track kind="captions" />
				</audio>
			{:else if block.type === 'video'}
				<!-- svelte-ignore a11y_media_has_caption -->
				<video
					controls
					src={`/api/v1/attachments/${block.conversation_attachment_id}/content`}
					class="max-h-96 max-w-full rounded-md"
				></video>
			{/if}
		{/each}
	</div>
{/if}

{#if streaming}<span
		class="ml-0.5 inline-block h-4 w-0.5 animate-pulse bg-zinc-900 dark:bg-zinc-100"
	></span>{/if}
