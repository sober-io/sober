<script lang="ts">
	import SlashCommandPalette from './SlashCommandPalette.svelte';

	interface Props {
		onsend: (content: string) => void;
		busy?: boolean;
		value?: string;
		onSlashCommand?: (command: string) => void;
	}

	let { onsend, busy = false, value = $bindable(''), onSlashCommand }: Props = $props();

	const showSlashCommands = $derived(value.startsWith('/'));

	const handleKeydown = (e: KeyboardEvent) => {
		if (e.key === 'Enter' && !e.shiftKey) {
			if (showSlashCommands) return; // let palette handle Enter
			e.preventDefault();
			submit();
		}
	};

	const submit = () => {
		const trimmed = value.trim();
		if (!trimmed) return;
		if (trimmed.startsWith('/') && onSlashCommand) {
			// intercept slash commands
			onSlashCommand(trimmed.split(' ')[0]);
			value = '';
			return;
		}
		onsend(trimmed);
		value = '';
	};

	const handleSlashExecute = (command: string) => {
		if (onSlashCommand) onSlashCommand(command);
		value = '';
	};

	const handleSlashClose = () => {
		value = '';
	};
</script>

<div class="relative border-t border-zinc-200 bg-white dark:border-zinc-800 dark:bg-zinc-950">
	{#if showSlashCommands}
		<SlashCommandPalette query={value} onExecute={handleSlashExecute} onClose={handleSlashClose} />
	{/if}
	<div class="flex gap-2 p-4">
		<textarea
			bind:value
			onkeydown={handleKeydown}
			placeholder="Send a message... (/ for commands)"
			rows="1"
			class="flex-1 resize-none rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400 dark:focus:ring-zinc-400"
		></textarea>
		<button
			onclick={submit}
			disabled={!value.trim()}
			class="rounded-md bg-zinc-900 px-4 py-2 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
		>
			{busy ? 'Queue' : 'Send'}
		</button>
	</div>
</div>
