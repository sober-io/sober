<script lang="ts">
	import { onMount } from 'svelte';

	interface Props {
		open: boolean;
		title: string;
		message: string;
		confirmText?: string;
		destructive?: boolean;
		onConfirm: () => void;
		onCancel: () => void;
	}

	let {
		open,
		title,
		message,
		confirmText = 'Confirm',
		destructive = false,
		onConfirm,
		onCancel
	}: Props = $props();

	let dialog: HTMLDialogElement | undefined = $state();

	$effect(() => {
		if (!dialog) return;
		if (open) {
			dialog.showModal();
		} else {
			dialog.close();
		}
	});

	const handleKeydown = (e: KeyboardEvent) => {
		if (e.key === 'Escape') {
			e.preventDefault();
			onCancel();
		}
	};
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<dialog
	bind:this={dialog}
	onkeydown={handleKeydown}
	onclose={onCancel}
	class="m-auto w-full max-w-sm rounded-lg border border-zinc-200 bg-white p-6 shadow-xl dark:border-zinc-700 dark:bg-zinc-900 backdrop:bg-black/50"
>
	<h2 class="mb-2 text-base font-semibold text-zinc-900 dark:text-zinc-100">{title}</h2>
	<p class="mb-6 text-sm text-zinc-600 dark:text-zinc-400">{message}</p>
	<div class="flex justify-end gap-3">
		<button
			onclick={onCancel}
			class="rounded-md border border-zinc-300 px-4 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-50 dark:border-zinc-600 dark:text-zinc-300 dark:hover:bg-zinc-800"
		>
			Cancel
		</button>
		<button
			onclick={onConfirm}
			class={[
				'rounded-md px-4 py-2 text-sm font-medium text-white',
				destructive
					? 'bg-red-600 hover:bg-red-700 dark:bg-red-700 dark:hover:bg-red-600'
					: 'bg-zinc-900 hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200'
			]}
		>
			{confirmText}
		</button>
	</div>
</dialog>
