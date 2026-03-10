<script lang="ts">
	interface Props {
		onsend: (content: string) => void;
		disabled?: boolean;
	}

	let { onsend, disabled = false }: Props = $props();
	let value = $state('');

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter' && !e.shiftKey) {
			e.preventDefault();
			submit();
		}
	}

	function submit() {
		const trimmed = value.trim();
		if (!trimmed || disabled) return;
		onsend(trimmed);
		value = '';
	}
</script>

<div class="flex gap-2 border-t border-zinc-200 bg-white p-4 dark:border-zinc-800 dark:bg-zinc-950">
	<textarea
		bind:value
		onkeydown={handleKeydown}
		{disabled}
		placeholder="Send a message..."
		rows="1"
		class="flex-1 resize-none rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none disabled:opacity-50 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400 dark:focus:ring-zinc-400"
	></textarea>
	<button
		onclick={submit}
		disabled={disabled || !value.trim()}
		class="rounded-md bg-zinc-900 px-4 py-2 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
	>
		Send
	</button>
</div>
