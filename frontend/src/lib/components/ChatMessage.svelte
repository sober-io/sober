<script lang="ts">
	import type { ToolCall } from '$lib/types';
	import StreamingText from './StreamingText.svelte';
	import ToolCallDisplay from './ToolCallDisplay.svelte';

	interface Props {
		role: 'User' | 'Assistant' | 'System';
		content: string;
		toolCalls?: ToolCall[];
		streaming?: boolean;
	}

	let { role, content, toolCalls, streaming = false }: Props = $props();

	const isUser = $derived(role === 'User');
</script>

<div class={['flex', isUser ? 'justify-end' : 'justify-start']}>
	<div
		class={[
			'max-w-[80%] rounded-lg px-4 py-2 text-sm',
			isUser
				? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900'
				: 'bg-zinc-100 text-zinc-900 dark:bg-zinc-800 dark:text-zinc-100'
		]}
	>
		{#if streaming}
			<StreamingText {content} {streaming} />
		{:else}
			<div class="whitespace-pre-wrap">{content}</div>
		{/if}

		{#if toolCalls}
			{#each toolCalls as tc (tc.name)}
				<ToolCallDisplay toolName={tc.name} input={tc.input} output={tc.output} />
			{/each}
		{/if}
	</div>
</div>
