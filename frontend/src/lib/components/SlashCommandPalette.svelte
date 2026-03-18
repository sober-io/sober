<script lang="ts">
	import type { SkillInfo } from '$lib/types';

	interface Command {
		name: string;
		description: string;
		isSkill?: boolean;
	}

	interface Props {
		query: string;
		onExecute: (command: string) => void;
		onClose: () => void;
		skills?: SkillInfo[];
	}

	const BUILTIN_COMMANDS: Command[] = [
		{ name: '/help', description: 'Show available commands' },
		{ name: '/info', description: 'Show conversation info' },
		{ name: '/clear', description: 'Clear all messages' }
	];

	let { query, onExecute, onClose, skills = [] }: Props = $props();

	const commands = $derived([
		...BUILTIN_COMMANDS,
		...skills.map((s) => ({
			name: `/${s.name}`,
			description: s.description,
			isSkill: true
		}))
	]);

	const filtered = $derived(
		commands.filter((c) => c.name.startsWith(query.length > 0 ? query : '/'))
	);

	let selectedIndex = $state(0);

	$effect(() => {
		void filtered;
		selectedIndex = 0;
	});

	const execute = (command: string) => {
		onExecute(command);
	};

	const handleKeydown = (e: KeyboardEvent) => {
		if (e.key === 'ArrowDown') {
			e.preventDefault();
			selectedIndex = (selectedIndex + 1) % filtered.length;
		} else if (e.key === 'ArrowUp') {
			e.preventDefault();
			selectedIndex = (selectedIndex - 1 + filtered.length) % filtered.length;
		} else if (e.key === 'Enter') {
			e.preventDefault();
			if (filtered[selectedIndex]) {
				execute(filtered[selectedIndex].name);
			}
		} else if (e.key === 'Escape') {
			e.preventDefault();
			onClose();
		}
	};
</script>

<svelte:window onkeydown={handleKeydown} />

{#if filtered.length > 0}
	<div
		class="absolute bottom-full left-0 right-0 z-50 mb-1 overflow-hidden rounded-lg border border-zinc-200 bg-white shadow-lg dark:border-zinc-700 dark:bg-zinc-900"
	>
		{#each filtered as command, i (command.name)}
			<button
				onclick={() => execute(command.name)}
				class={[
					'flex w-full items-center gap-3 px-4 py-2.5 text-left text-sm transition-colors',
					i === selectedIndex
						? 'bg-zinc-100 dark:bg-zinc-800'
						: 'hover:bg-zinc-50 dark:hover:bg-zinc-800/50'
				]}
			>
				<span class="font-mono font-medium text-zinc-900 dark:text-zinc-100">{command.name}</span>
				{#if command.isSkill}
					<span class="rounded bg-emerald-100 px-1.5 py-0.5 text-xs font-medium text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400">skill</span>
				{/if}
				<span class="text-zinc-500 dark:text-zinc-400">{command.description}</span>
			</button>
		{/each}
	</div>
{/if}
