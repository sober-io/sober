<script lang="ts">
	import type { ToolCall } from '$lib/types';
	import { renderMarkdown } from '$lib/utils/markdown';
	import StreamingText from './StreamingText.svelte';
	import ToolCallDisplay from './ToolCallDisplay.svelte';
	import ThinkingIndicator from './ThinkingIndicator.svelte';

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
		onDelete
	}: Props = $props();

	const isUser = $derived(role === 'user');
	const hasToolCalls = $derived(toolCalls && toolCalls.length > 0);
	const hasThinkingContent = $derived(thinkingContent.length > 0);
	const renderedContent = $derived(content ? renderMarkdown(content) : '');
	const sourceLabel = $derived(source && source !== 'human' ? source : undefined);
	const canDelete = $derived(isUser && !!onDelete && !streaming && !ephemeral);
</script>

<div class={['flex', isUser ? 'justify-end' : 'justify-start', ephemeral && 'opacity-75']}>
	<div class={['group flex max-w-[80%] flex-col', isUser ? 'items-end' : 'items-start']}>
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
		<div class="flex items-center gap-2">
			{#if timestamp}
				<span class="mt-0.5 text-[10px] text-zinc-400 dark:text-zinc-400">{timestamp}</span>
			{/if}
			{#if canDelete}
				<button
					onclick={onDelete}
					class="mt-0.5 text-[10px] text-zinc-300 opacity-0 transition-opacity hover:text-red-400 group-hover:opacity-100 dark:text-zinc-600 dark:hover:text-red-500"
					title="Delete message"
					aria-label="Delete message"
				>
					Delete
				</button>
			{/if}
		</div>
	</div>
</div>
