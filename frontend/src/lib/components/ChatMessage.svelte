<script lang="ts">
	import type { Tag, ToolCall } from '$lib/types';
	import { renderMarkdown } from '$lib/utils/markdown';
	import StreamingText from './StreamingText.svelte';
	import ToolCallDisplay from './ToolCallDisplay.svelte';
	import ThinkingIndicator from './ThinkingIndicator.svelte';
	import MessageActionBar from './MessageActionBar.svelte';
	import MessageTagPopover from './MessageTagPopover.svelte';

	interface Props {
		role: 'user' | 'assistant' | 'system';
		content: string;
		thinkingContent?: string;
		toolCalls?: ToolCall[];
		streaming?: boolean;
		thinking?: boolean;
		timestamp?: string;
		source?: string;
		ephemeral?: boolean;
		messageId?: string;
		tags?: Tag[];
		onTagsChange?: (tags: Tag[]) => void;
		onDelete?: () => void;
	}

	let {
		role,
		content,
		thinkingContent = '',
		toolCalls,
		streaming = false,
		thinking = false,
		timestamp,
		source,
		ephemeral = false,
		messageId,
		tags,
		onTagsChange,
		onDelete
	}: Props = $props();

	const isUser = $derived(role === 'user');
	const hasToolCalls = $derived(toolCalls && toolCalls.length > 0);
	const hasThinkingContent = $derived(thinkingContent.length > 0);
	const renderedContent = $derived(content ? renderMarkdown(content) : '');
	const sourceLabel = $derived(source && source !== 'human' ? source : undefined);
	const canDelete = $derived(isUser && !!onDelete && !streaming && !ephemeral);
	const canShowActions = $derived(!streaming && !ephemeral && !thinking);
	const hasTags = $derived(tags && tags.length > 0);

	let showTagPopover = $state(false);
	let showAllTags = $state(false);

	const visibleTags = $derived.by(() => {
		if (!tags || tags.length === 0) return [];
		if (showAllTags || tags.length < 3) return tags;
		return tags.slice(0, 2);
	});

	const hiddenTagCount = $derived(tags && tags.length >= 3 && !showAllTags ? tags.length - 2 : 0);
</script>

<div class={['flex', isUser ? 'justify-end' : 'justify-start', ephemeral && 'opacity-75']}>
	<div class={['group relative flex max-w-[80%] flex-col', isUser ? 'items-end' : 'items-start']}>
		{#if sourceLabel}
			<span
				class="mb-0.5 text-[10px] font-medium uppercase tracking-wider text-zinc-400 dark:text-zinc-500"
				>{sourceLabel}</span
			>
		{/if}
		<div
			class={[
				'rounded-lg px-4 py-2 text-sm',
				isUser
					? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900'
					: sourceLabel
						? 'border border-zinc-200 bg-zinc-50 text-zinc-900 dark:border-zinc-700 dark:bg-zinc-800/50 dark:text-zinc-100'
						: 'bg-zinc-100 text-zinc-900 dark:bg-zinc-800 dark:text-zinc-100'
			]}
		>
			{#if thinking && !content}
				<ThinkingIndicator />
			{:else if streaming}
				<StreamingText {content} {streaming} />
			{:else}
				<!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized markdown rendering -->
				<div class="chat-prose prose prose-sm max-w-none">{@html renderedContent}</div>
			{/if}

			{#if hasThinkingContent}
				<details class="mt-2 border-t border-zinc-200 pt-2 dark:border-zinc-700">
					<summary class="cursor-pointer text-xs text-zinc-500 dark:text-zinc-400">
						{thinking ? 'Reasoning...' : 'Reasoning'}
					</summary>
					<div
						class="mt-1 max-h-60 overflow-y-auto whitespace-pre-wrap text-xs text-zinc-500 dark:text-zinc-400"
					>
						{thinkingContent}
					</div>
				</details>
			{/if}

			{#if thinking && hasToolCalls}
				<details open class="mt-2 border-t border-zinc-200 pt-2 dark:border-zinc-700">
					<summary class="cursor-pointer text-xs text-zinc-500 dark:text-zinc-400">
						Thinking...
					</summary>
					<div class="mt-1">
						{#each toolCalls! as tc (tc.id)}
							<ToolCallDisplay
								toolName={tc.name}
								input={tc.input}
								output={tc.output}
								loading={!tc.output}
							/>
						{/each}
					</div>
				</details>
			{:else if hasToolCalls}
				{#each toolCalls! as tc (tc.id)}
					<ToolCallDisplay
						toolName={tc.name}
						input={tc.input}
						output={tc.output}
						loading={!tc.output}
					/>
				{/each}
			{/if}
		</div>

		{#if hasTags}
			<div class={['mt-1 flex flex-wrap gap-1', isUser ? 'justify-end' : 'justify-start']}>
				{#each visibleTags as tag (tag.id)}
					<span
						class="inline-flex items-center rounded-full px-1.5 py-0 text-[10px] font-medium"
						style="background-color: color-mix(in srgb, {tag.color} 15%, transparent); color: {tag.color}; border: 1px solid color-mix(in srgb, {tag.color} 30%, transparent);"
					>
						{tag.name}
					</span>
				{/each}
				{#if hiddenTagCount > 0}
					<button
						onclick={() => (showAllTags = true)}
						class="inline-flex items-center rounded-full border border-zinc-200 bg-zinc-50 px-1.5 py-0 text-[10px] font-medium text-zinc-500 hover:bg-zinc-100 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-400 dark:hover:bg-zinc-700"
					>
						+{hiddenTagCount} more
					</button>
				{/if}
			</div>
		{/if}

		{#if canShowActions}
			<div
				class={[
					'absolute -top-3 opacity-0 transition-opacity group-hover:opacity-100',
					isUser ? 'right-0' : 'left-0'
				]}
			>
				<MessageActionBar
					onTag={() => (showTagPopover = !showTagPopover)}
					onDelete={canDelete ? onDelete : undefined}
				/>
			</div>
		{/if}

		{#if showTagPopover && messageId && onTagsChange}
			<div class={['absolute top-6 z-50', isUser ? 'right-0' : 'left-0']}>
				<MessageTagPopover
					{messageId}
					tags={tags ?? []}
					{onTagsChange}
					onClose={() => (showTagPopover = false)}
				/>
			</div>
		{/if}

		<div class="flex items-center gap-2">
			{#if timestamp}
				<span class="mt-0.5 text-[10px] text-zinc-400 dark:text-zinc-400">{timestamp}</span>
			{/if}
		</div>
	</div>
</div>
