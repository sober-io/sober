<script lang="ts">
	import type { ContentBlock, ConversationAttachment, Tag, ToolExecution } from '$lib/types';
	import { renderMarkdown, highlighterReady } from '$lib/utils/markdown.svelte';
	import StreamingText from './StreamingText.svelte';
	import ContentBlockRenderer from './ContentBlockRenderer.svelte';
	import ToolCallDisplay from './ToolCallDisplay.svelte';
	import ThinkingIndicator from './ThinkingIndicator.svelte';
	import MessageActionBar from './MessageActionBar.svelte';
	import MessageTagPopover from './MessageTagPopover.svelte';

	interface Props {
		role: 'user' | 'assistant' | 'system' | 'event';
		content: string;
		contentBlocks?: ContentBlock[];
		attachments?: Record<string, ConversationAttachment>;
		thinkingContent?: string;
		toolExecutions?: ToolExecution[];
		streaming?: boolean;
		thinking?: boolean;
		timestamp?: string;
		source?: string;
		ephemeral?: boolean;
		messageId?: string;
		senderUsername?: string;
		/** True for messages created during this session (not loaded from DB). */
		live?: boolean;
		tags?: Tag[];
		onTagsChange?: (tags: Tag[]) => void;
		onDelete?: () => void;
	}

	let {
		role,
		content,
		contentBlocks,
		attachments,
		thinkingContent = '',
		toolExecutions,
		streaming = false,
		thinking = false,
		timestamp,
		source,
		ephemeral = false,
		live = false,
		messageId,
		senderUsername,
		tags,
		onTagsChange,
		onDelete
	}: Props = $props();

	const isUser = $derived(role === 'user');
	const hasToolExecutions = $derived(toolExecutions && toolExecutions.length > 0);
	const hasThinkingContent = $derived(thinkingContent.length > 0);
	const hasNonTextBlocks = $derived(
		contentBlocks ? contentBlocks.some((b) => b.type !== 'text') : false
	);
	const renderedContent = $derived(
		// Read highlighterReady.version to re-derive when shiki finishes loading
		content ? (highlighterReady.version, renderMarkdown(content)) : ''
	);
	const sourceLabel = $derived(
		source && source !== 'human' && source !== 'web' ? source : undefined
	);
	const runningToolCount = $derived(
		toolExecutions?.filter((te) => te.status === 'pending' || te.status === 'running').length ?? 0
	);
	const canDelete = $derived(isUser && !!onDelete && !streaming && !ephemeral);
	const canShowActions = $derived(!streaming && !ephemeral && !thinking);
	const hasTags = $derived(tags && tags.length > 0);

	/** Compute tool execution duration from client-side timestamps or DB timestamps. */
	function getToolDurationMs(te: ToolExecution): number | undefined {
		if (te._durationMs !== undefined) return te._durationMs;
		if (te.started_at && te.completed_at) {
			return new Date(te.completed_at).getTime() - new Date(te.started_at).getTime();
		}
		return undefined;
	}

	let showTagPopover = $state(false);
	let showAllTags = $state(false);
	let reasoningExpanded = $state(live);

	const visibleTags = $derived.by(() => {
		if (!tags || tags.length === 0) return [];
		if (showAllTags || tags.length < 3) return tags;
		return tags.slice(0, 2);
	});

	const hiddenTagCount = $derived(tags && tags.length >= 3 && !showAllTags ? tags.length - 2 : 0);
</script>

{#if role === 'event'}
	<div class="flex justify-center py-1">
		<span class="text-xs text-zinc-400 dark:text-zinc-500 italic">{content}</span>
	</div>
{:else}
	<div class={['flex', isUser ? 'justify-end' : 'justify-start', ephemeral && 'opacity-75']}>
		<div
			class={[
				'group relative flex max-w-[80%] min-w-0 flex-col',
				isUser ? 'items-end' : 'items-start'
			]}
		>
			{#if senderUsername}
				<span class="mb-0.5 text-xs font-medium text-zinc-500 dark:text-zinc-400"
					>{senderUsername}</span
				>
			{/if}
			{#if sourceLabel}
				<span
					class="mb-0.5 text-[10px] font-medium uppercase tracking-wider text-zinc-400 dark:text-zinc-500"
					>{sourceLabel}</span
				>
			{/if}
			<div
				class={[
					'min-w-0 overflow-hidden rounded-lg px-4 py-2 text-sm break-words',
					isUser
						? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900'
						: sourceLabel
							? 'border border-zinc-200 bg-zinc-50 text-zinc-900 dark:border-zinc-700 dark:bg-zinc-800/50 dark:text-zinc-100'
							: 'bg-zinc-100 text-zinc-900 dark:bg-zinc-800 dark:text-zinc-100'
				]}
			>
				{#if thinking && !content}
					<ThinkingIndicator {thinkingContent} />
				{:else if streaming}
					<StreamingText {content} {streaming} />
				{:else if hasNonTextBlocks && contentBlocks}
					<ContentBlockRenderer blocks={contentBlocks} {attachments} />
				{:else}
					<!-- eslint-disable-next-line svelte/no-at-html-tags -- DOMPurify-sanitized in renderMarkdown -->
					<div class="chat-prose prose prose-sm max-w-none">{@html renderedContent}</div>
				{/if}

				{#if hasThinkingContent && !thinking}
					<div
						class={[
							'my-2 rounded-md border text-sm',
							streaming
								? 'animate-reasoning-pulse border-zinc-300 dark:border-zinc-600'
								: 'border-zinc-200 dark:border-zinc-700'
						]}
					>
						<button
							onclick={() => (reasoningExpanded = !reasoningExpanded)}
							class="flex w-full items-center gap-2 px-3 py-2 text-left text-zinc-500 hover:bg-zinc-50 dark:text-zinc-400 dark:hover:bg-zinc-800/50"
						>
							<svg
								class="h-3 w-3"
								fill="none"
								viewBox="0 0 24 24"
								stroke="currentColor"
								stroke-width="2"
							>
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									d="M9.663 17h4.673M12 3v1m6.364 1.636l-.707.707M21 12h-1M4 12H3m3.343-5.657l-.707-.707m2.828 9.9a5 5 0 117.072 0l-.548.547A3.374 3.374 0 0014 18.469V19a2 2 0 11-4 0v-.531c0-.895-.356-1.754-.988-2.386l-.548-.547z"
								/>
							</svg>
							<span class="font-mono text-xs">reasoning</span>
							<svg
								class={[
									'ml-auto h-3 w-3 shrink-0 transition-transform',
									reasoningExpanded && 'rotate-90'
								]}
								fill="currentColor"
								viewBox="0 0 20 20"
							>
								<path d="M6 4l8 6-8 6V4z" />
							</svg>
						</button>
						{#if reasoningExpanded}
							<div class="border-t border-zinc-200 px-3 py-2 dark:border-zinc-700">
								<pre
									class="max-h-60 max-w-full overflow-auto whitespace-pre-wrap break-words text-xs text-zinc-500 dark:text-zinc-400">{thinkingContent}</pre>
							</div>
						{/if}
					</div>
				{/if}

				{#if hasToolExecutions}
					{#if runningToolCount > 0}
						<div class="mt-2 flex items-center gap-2 text-xs text-zinc-400 dark:text-zinc-500">
							<span
								class="inline-block h-3 w-3 animate-spin rounded-full border-2 border-zinc-400 border-t-transparent"
							></span>
							<span class="animate-pulse"
								>{runningToolCount} tool{runningToolCount > 1 ? 's' : ''} running</span
							>
						</div>
					{/if}
					{#each toolExecutions! as te (te.id)}
						<ToolCallDisplay
							toolName={te.tool_name}
							input={te.input}
							output={te.output}
							loading={te.status === 'pending' || te.status === 'running'}
							isError={te.status === 'failed'}
							error={te.error}
							durationMs={getToolDurationMs(te)}
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
{/if}
