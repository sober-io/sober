<script lang="ts">
	interface Props {
		onTag: () => void;
		onDelete?: () => void;
	}

	let { onTag, onDelete }: Props = $props();
</script>

<div
	class="inline-flex items-center gap-0.5 rounded-md border border-zinc-200 bg-white px-1 py-0.5 shadow-sm dark:border-zinc-700 dark:bg-zinc-800"
>
	<button
		onclick={onTag}
		title="Tag message"
		class="flex items-center justify-center rounded p-1 text-zinc-500 hover:bg-zinc-100 hover:text-zinc-700 dark:text-zinc-400 dark:hover:bg-zinc-700 dark:hover:text-zinc-200"
	>
		<svg class="h-3.5 w-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
			<path
				stroke-linecap="round"
				stroke-linejoin="round"
				stroke-width="2"
				d="M7 7h.01M7 3h5c.512 0 1.024.195 1.414.586l7 7a2 2 0 010 2.828l-7 7a2 2 0 01-2.828 0l-7-7A1.994 1.994 0 013 12V7a4 4 0 014-4z"
			/>
		</svg>
	</button>

	{#if onDelete}
		<button
			onclick={onDelete}
			title="Delete message"
			class="flex items-center justify-center rounded p-1 text-zinc-500 hover:bg-red-50 hover:text-red-600 dark:text-zinc-400 dark:hover:bg-red-900/20 dark:hover:text-red-400"
		>
			<svg class="h-3.5 w-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
				<path
					stroke-linecap="round"
					stroke-linejoin="round"
					stroke-width="2"
					d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
				/>
			</svg>
		</button>
	{/if}
</div>
