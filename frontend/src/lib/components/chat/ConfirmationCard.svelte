<script lang="ts">
	import type { ConfirmRequest } from '$lib/types';

	interface Props {
		request: ConfirmRequest;
		onRespond: (confirmId: string, approved: boolean) => void;
	}

	let { request, onRespond }: Props = $props();

	const riskColors: Record<string, string> = {
		safe: 'bg-emerald-500/20 text-emerald-400 border-emerald-500/30',
		moderate: 'bg-amber-500/20 text-amber-400 border-amber-500/30',
		dangerous: 'bg-red-500/20 text-red-400 border-red-500/30'
	};

	const riskBadge = $derived(riskColors[request.risk_level] ?? riskColors.moderate);
</script>

<div
	class="flex w-full items-center gap-3 rounded-lg border border-zinc-700 bg-zinc-800 px-4 py-2.5 shadow-lg shadow-black/20"
>
	<div class="min-w-0 flex-1">
		<div class="flex items-center gap-2">
			<span class="rounded border px-1.5 py-0.5 text-[10px] font-medium {riskBadge}">
				{request.risk_level}
			</span>
			<code class="truncate text-sm text-zinc-200">{request.command}</code>
			{#if request.reason}
				<span class="hidden truncate text-xs text-zinc-500 sm:inline">{request.reason}</span>
			{/if}
		</div>
	</div>

	<div class="flex shrink-0 gap-1.5">
		<button
			onclick={() => onRespond(request.confirm_id, true)}
			class="rounded bg-emerald-600 px-2.5 py-1 text-xs font-medium text-white transition-colors hover:bg-emerald-500"
		>
			Allow
		</button>
		<button
			onclick={() => onRespond(request.confirm_id, false)}
			class="rounded bg-zinc-600 px-2.5 py-1 text-xs font-medium text-zinc-200 transition-colors hover:bg-zinc-500"
		>
			Deny
		</button>
	</div>
</div>
