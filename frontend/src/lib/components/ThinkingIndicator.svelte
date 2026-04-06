<script lang="ts">
	interface Props {
		thinkingContent?: string;
	}

	let { thinkingContent = '' }: Props = $props();

	let scrollContainer: HTMLDivElement | undefined = $state();

	$effect(() => {
		// Auto-scroll to bottom as reasoning content streams in.
		if (thinkingContent && scrollContainer) {
			scrollContainer.scrollTop = scrollContainer.scrollHeight;
		}
	});
</script>

<div role="status" aria-label="Assistant is thinking">
	<div class="flex items-center gap-1.5 py-1">
		<span class="dot"></span>
		<span class="dot [animation-delay:150ms]"></span>
		<span class="dot [animation-delay:300ms]"></span>
		<span class="ml-1.5 text-xs text-zinc-400 dark:text-zinc-500">Thinking…</span>
	</div>

	{#if thinkingContent}
		<div
			bind:this={scrollContainer}
			class="mt-1 max-h-40 overflow-y-auto break-words border-t border-zinc-200 pt-1.5 text-xs leading-relaxed text-zinc-400 whitespace-pre-wrap dark:border-zinc-700 dark:text-zinc-500"
		>
			{thinkingContent}
		</div>
	{/if}
</div>

<style>
	.dot {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background-color: currentColor;
		opacity: 0.5;
		animation: pulse 1.2s ease-in-out infinite;
	}

	@keyframes pulse {
		0%,
		60%,
		100% {
			opacity: 0.4;
			transform: scale(1);
		}
		30% {
			opacity: 1;
			transform: scale(1.3);
		}
	}
</style>
