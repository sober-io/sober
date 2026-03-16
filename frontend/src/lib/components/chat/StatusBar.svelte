<script lang="ts">
	import type { PermissionMode } from '$lib/types';
	import { PERMISSION_MODES } from '$lib/constants/permission-modes';
	import PermissionModeSelector from '$lib/components/PermissionModeSelector.svelte';

	interface Props {
		mode: PermissionMode;
		onModeChange: (mode: PermissionMode) => void;
	}

	let { mode, onModeChange }: Props = $props();

	const modeOrder: PermissionMode[] = PERMISSION_MODES.map((m) => m.value);

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
</script>

<svelte:window onkeydown={handleKeydown} />

<div
	class="flex items-center justify-between border-t border-zinc-800 bg-zinc-900/50 px-3 py-1.5 text-xs"
>
	<PermissionModeSelector {mode} {onModeChange} />
	<span class="text-zinc-600">Ctrl+Shift+P</span>
</div>
