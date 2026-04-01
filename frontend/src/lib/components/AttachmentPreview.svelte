<script lang="ts">
	import type { AttachmentState } from '$lib/stores/uploads.svelte';

	interface Props {
		attachments: Map<string, AttachmentState>;
		onremove: (id: string) => void;
	}

	let { attachments, onremove }: Props = $props();

	const formatSize = (bytes: number): string => {
		if (bytes < 1024) return `${bytes} B`;
		if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
		return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
	};
</script>

{#if attachments.size > 0}
	<div class="flex flex-wrap gap-2 border-t border-zinc-200 px-4 pt-2 dark:border-zinc-800">
		{#each [...attachments] as [id, state] (id)}
			<div
				class="relative flex items-center gap-2 rounded-md border border-zinc-200 bg-zinc-50 px-2 py-1.5 text-xs dark:border-zinc-700 dark:bg-zinc-800"
			>
				{#if state.previewUrl}
					<img
						src={state.previewUrl}
						alt={state.file.name}
						class="h-10 w-10 rounded object-cover"
					/>
				{:else}
					<div
						class="flex h-10 w-10 items-center justify-center rounded bg-zinc-200 text-zinc-500 dark:bg-zinc-700 dark:text-zinc-400"
					>
						<svg class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="2"
								d="M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z"
							/>
						</svg>
					</div>
				{/if}

				<div class="max-w-[120px]">
					<div class="truncate font-medium text-zinc-700 dark:text-zinc-300">
						{state.file.name}
					</div>
					<div class="text-zinc-400 dark:text-zinc-500">
						{formatSize(state.file.size)}
					</div>
				</div>

				{#if state.status === 'uploading'}
					<div class="absolute inset-0 flex items-center justify-center rounded-md bg-black/30">
						<div
							class="h-4 w-4 animate-spin rounded-full border-2 border-white border-t-transparent"
						></div>
					</div>
				{/if}

				{#if state.status === 'failed'}
					<div
						class="absolute inset-0 flex items-center justify-center rounded-md bg-red-500/20"
						title={state.error}
					>
						<svg class="h-5 w-5 text-red-500" fill="none" viewBox="0 0 24 24" stroke="currentColor">
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="2"
								d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z"
							/>
						</svg>
					</div>
				{/if}

				<button
					onclick={() => onremove(id)}
					title="Remove attachment"
					class="absolute -top-1.5 -right-1.5 flex h-4 w-4 items-center justify-center rounded-full bg-zinc-500 text-white hover:bg-zinc-600 dark:bg-zinc-400 dark:text-zinc-900 dark:hover:bg-zinc-300"
				>
					<svg class="h-2.5 w-2.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
						<path
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-width="3"
							d="M6 18L18 6M6 6l12 12"
						/>
					</svg>
				</button>
			</div>
		{/each}
	</div>
{/if}
