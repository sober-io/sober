<script lang="ts">
	interface Props {
		onAdd: (username: string) => void;
	}

	let { onAdd }: Props = $props();

	let username = $state('');

	function handleSubmit() {
		const trimmed = username.trim();
		if (trimmed) {
			onAdd(trimmed);
			username = '';
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			e.preventDefault();
			handleSubmit();
		}
	}
</script>

<div class="flex gap-2">
	<input
		type="text"
		bind:value={username}
		onkeydown={handleKeydown}
		placeholder="Add by username..."
		class="min-w-0 flex-1 rounded-md border border-zinc-300 bg-transparent px-3 py-1.5 text-sm text-zinc-900 outline-none placeholder:text-zinc-400 focus:border-zinc-500 dark:border-zinc-600 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400"
	/>
	<button
		onclick={handleSubmit}
		disabled={!username.trim()}
		class="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-zinc-700 disabled:opacity-40 disabled:hover:bg-zinc-900 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-300 dark:disabled:hover:bg-zinc-100"
	>
		Add
	</button>
</div>
