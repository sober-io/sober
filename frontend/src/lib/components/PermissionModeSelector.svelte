<script lang="ts">
	import type { PermissionMode } from '$lib/types';
	import { PERMISSION_MODES } from '$lib/constants/permission-modes';

	interface Props {
		mode: PermissionMode;
		onModeChange: (mode: PermissionMode) => void;
		compact?: boolean;
	}

	let { mode, onModeChange, compact = false }: Props = $props();

	function buttonClass(m: (typeof PERMISSION_MODES)[number]): string {
		if (mode !== m.value) {
			return compact
				? 'text-zinc-500 hover:text-zinc-300'
				: 'text-zinc-500 hover:text-zinc-700 dark:text-zinc-400 dark:hover:text-zinc-200';
		}
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

<div
	class={compact
		? 'flex items-center gap-1 rounded-md bg-zinc-800 p-0.5'
		: 'flex w-full gap-1 rounded-lg bg-zinc-100 p-1 dark:bg-zinc-800'}
>
	{#each PERMISSION_MODES as m (m.value)}
		<button
			onclick={() => onModeChange(m.value)}
			class={compact
				? 'rounded px-2 py-1 text-xs transition-colors ' + buttonClass(m)
				: 'flex-1 rounded-md px-3 py-2 text-sm font-medium transition-colors ' + buttonClass(m)}
			title={m.description}
		>
			{m.label}
		</button>
	{/each}
</div>
