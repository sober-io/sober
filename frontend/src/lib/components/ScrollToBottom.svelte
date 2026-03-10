<script lang="ts">
	import { fade } from 'svelte/transition';

	interface Props {
		onclick: () => void;
		visible: boolean;
	}

	let { onclick, visible }: Props = $props();
</script>

{#if visible}
	<button
		transition:fade={{ duration: 150 }}
		{onclick}
		aria-label="Scroll to bottom"
		class="absolute bottom-4 left-1/2 -translate-x-1/2 rounded-full border border-zinc-200 bg-white p-2 shadow-md hover:bg-zinc-50 dark:border-zinc-700 dark:bg-zinc-800 dark:hover:bg-zinc-700"
	>
		<svg
			xmlns="http://www.w3.org/2000/svg"
			width="16"
			height="16"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			stroke-width="2"
			stroke-linecap="round"
			stroke-linejoin="round"
			class="text-zinc-600 dark:text-zinc-300"
		>
			<polyline points="6 9 12 15 18 9"></polyline>
		</svg>
	</button>
{/if}
