<div role="status" aria-label="Assistant is thinking" class="flex items-center gap-1.5 py-1">
	<span class="dot"></span>
	<span class="dot [animation-delay:150ms]"></span>
	<span class="dot [animation-delay:300ms]"></span>
	<span class="ml-1.5 text-xs text-zinc-400 dark:text-zinc-500">Thinking…</span>
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
