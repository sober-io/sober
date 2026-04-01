<script lang="ts">
	import type { ConversationAttachment } from '$lib/types';

	interface Props {
		attachmentId: string;
		attachment?: ConversationAttachment;
	}

	let { attachmentId, attachment }: Props = $props();

	const downloadUrl = $derived(`/api/v1/attachments/${attachmentId}/content`);

	const formatSize = (bytes: number): string => {
		if (bytes < 1024) return `${bytes} B`;
		if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
		return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
	};
</script>

<button
	type="button"
	onclick={() => window.open(downloadUrl, '_blank')}
	class="my-1 flex w-full cursor-pointer items-center gap-3 rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 text-left text-sm transition-colors hover:bg-zinc-100 dark:border-zinc-700 dark:bg-zinc-800/50 dark:hover:bg-zinc-800"
>
	<svg
		class="h-5 w-5 shrink-0 text-zinc-500 dark:text-zinc-400"
		fill="none"
		viewBox="0 0 24 24"
		stroke="currentColor"
	>
		<path
			stroke-linecap="round"
			stroke-linejoin="round"
			stroke-width="2"
			d="M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z"
		/>
	</svg>
	<div class="min-w-0 flex-1">
		<div class="truncate font-medium text-zinc-700 dark:text-zinc-300">
			{attachment?.filename ?? 'File'}
		</div>
		{#if attachment}
			<div class="text-xs text-zinc-400 dark:text-zinc-500">
				{formatSize(attachment.size)}
			</div>
		{/if}
	</div>
	<svg
		class="h-4 w-4 shrink-0 text-zinc-400 dark:text-zinc-500"
		fill="none"
		viewBox="0 0 24 24"
		stroke="currentColor"
	>
		<path
			stroke-linecap="round"
			stroke-linejoin="round"
			stroke-width="2"
			d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4"
		/>
	</svg>
</button>
