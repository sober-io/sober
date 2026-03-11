<script lang="ts">
	import type { ConfirmRequest } from '$lib/types';

	interface Props {
		request: ConfirmRequest;
		resolved?: 'approved' | 'denied';
		onRespond: (confirmId: string, approved: boolean) => void;
	}

	let { request, resolved, onRespond }: Props = $props();

	const riskColors: Record<string, string> = {
		safe: 'bg-emerald-500/20 text-emerald-400 border-emerald-500/30',
		moderate: 'bg-amber-500/20 text-amber-400 border-amber-500/30',
		dangerous: 'bg-red-500/20 text-red-400 border-red-500/30'
	};

	const riskBadge = $derived(riskColors[request.risk_level] ?? riskColors.moderate);
</script>

<div class="my-2 rounded-lg border border-zinc-700 bg-zinc-800/50 p-4">
	<div class="mb-3 flex items-center gap-2">
		<span class="text-sm font-medium text-zinc-300">Command Approval</span>
		<span class="rounded border px-2 py-0.5 text-xs font-medium {riskBadge}">
			{request.risk_level}
		</span>
	</div>

	<pre
		class="mb-2 overflow-x-auto rounded bg-zinc-900 px-3 py-2 font-mono text-sm text-zinc-200">{request.command}</pre>

	{#if request.affects.length > 0}
		<div class="mb-2 text-xs text-zinc-400">
			<span class="font-medium">Affects:</span>
			{#each request.affects as item}
				<span class="ml-1">{item}</span>
			{/each}
		</div>
	{/if}

	{#if request.reason}
		<p class="mb-3 text-xs text-zinc-500">{request.reason}</p>
	{/if}

	{#if resolved}
		<div
			class="text-sm font-medium {resolved === 'approved' ? 'text-emerald-400' : 'text-red-400'}"
		>
			{resolved === 'approved' ? 'Approved' : 'Denied'}
		</div>
	{:else}
		<div class="flex gap-2">
			<button
				onclick={() => onRespond(request.confirm_id, true)}
				class="rounded bg-emerald-600 px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-emerald-500"
			>
				Approve
			</button>
			<button
				onclick={() => onRespond(request.confirm_id, false)}
				class="rounded bg-zinc-600 px-3 py-1.5 text-sm font-medium text-zinc-200 transition-colors hover:bg-zinc-500"
			>
				Deny
			</button>
		</div>
	{/if}
</div>
