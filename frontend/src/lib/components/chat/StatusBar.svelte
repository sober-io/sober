<script lang="ts">
	interface Props {
		mode: 'interactive' | 'policy_based' | 'autonomous';
		onModeChange: (mode: 'interactive' | 'policy_based' | 'autonomous') => void;
	}

	let { mode, onModeChange }: Props = $props();

	type ModeValue = 'interactive' | 'policy_based' | 'autonomous';

	const modes: ReadonlyArray<{ value: ModeValue; label: string; color: string }> = [
		{ value: 'interactive', label: 'Interactive', color: 'emerald' },
		{ value: 'policy_based', label: 'Policy', color: 'amber' },
		{ value: 'autonomous', label: 'Autonomous', color: 'red' }
	];

	const modeOrder: ModeValue[] = ['interactive', 'policy_based', 'autonomous'];

	function cycleMode() {
		const idx = modeOrder.indexOf(mode);
		const next = modeOrder[(idx + 1) % modeOrder.length];
		onModeChange(next);
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.ctrlKey && e.shiftKey && e.key === 'P') {
			e.preventDefault();
			cycleMode();
		}
	}

	function modeButtonClass(m: { value: ModeValue; color: string }): string {
		if (mode !== m.value) return 'text-zinc-500 hover:text-zinc-300';
		switch (m.color) {
			case 'emerald':
				return 'bg-emerald-600/30 text-emerald-400';
			case 'amber':
				return 'bg-amber-600/30 text-amber-400';
			default:
				return 'bg-red-600/30 text-red-400';
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

<div
	class="flex items-center justify-between border-t border-zinc-800 bg-zinc-900/50 px-3 py-1.5 text-xs"
>
	<div class="flex items-center gap-1 rounded-md bg-zinc-800 p-0.5">
		{#each modes as m (m.value)}
			<button
				onclick={() => onModeChange(m.value)}
				class="rounded px-2 py-1 transition-colors {modeButtonClass(m)}"
			>
				{m.label}
			</button>
		{/each}
	</div>
	<span class="text-zinc-600">Ctrl+Shift+P</span>
</div>
