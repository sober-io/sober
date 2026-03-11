<div role="status" aria-label="Assistant is thinking" class="flex items-center gap-1 py-1">
	<span class="dot"></span>
	<span class="dot [animation-delay:150ms]"></span>
	<span class="dot [animation-delay:300ms]"></span>
</div>

<style>
	.dot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background-color: currentColor;
		opacity: 0.4;
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
			transform: scale(1.2);
		}
	}
</style>
