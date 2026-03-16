<script lang="ts">
	import type { Job } from '$lib/types';

	interface Props {
		jobs: Job[];
		loading: boolean;
	}

	let { jobs, loading }: Props = $props();

	const statusColors: Record<Job['status'], string> = {
		active: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400',
		paused: 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400',
		cancelled: 'bg-zinc-100 text-zinc-600 dark:bg-zinc-700 dark:text-zinc-400',
		running: 'bg-sky-100 text-sky-700 dark:bg-sky-900/30 dark:text-sky-400'
	};

	function formatNextRun(iso: string): string {
		const date = new Date(iso);
		const now = new Date();
		const diffMs = date.getTime() - now.getTime();

		if (diffMs < 0) return 'overdue';
		if (diffMs < 60_000) return 'in less than a minute';
		if (diffMs < 3_600_000) return `in ${Math.round(diffMs / 60_000)}m`;
		if (diffMs < 86_400_000) return `in ${Math.round(diffMs / 3_600_000)}h`;
		return date.toLocaleDateString();
	}
</script>

{#if loading}
	<p class="py-2 text-xs text-zinc-400 dark:text-zinc-500">Loading...</p>
{:else if jobs.length === 0}
	<p class="py-2 text-xs text-zinc-400 dark:text-zinc-500">
		No scheduled jobs for this conversation
	</p>
{:else}
	<ul class="space-y-2">
		{#each jobs as job (job.id)}
			<li
				class="flex items-center justify-between rounded-lg border border-zinc-200 px-3 py-2 dark:border-zinc-700"
			>
				<div class="min-w-0 flex-1">
					<p class="truncate text-sm font-medium text-zinc-900 dark:text-zinc-100">
						{job.name}
					</p>
					<p class="text-xs text-zinc-500 dark:text-zinc-400">{job.schedule}</p>
				</div>
				<div class="flex shrink-0 items-center gap-2">
					<span
						class={[
							'rounded-full px-2 py-0.5 text-[10px] font-medium',
							statusColors[job.status]
						]}
					>
						{job.status}
					</span>
					<span class="text-xs text-zinc-400 dark:text-zinc-500">
						{formatNextRun(job.next_run_at)}
					</span>
				</div>
			</li>
		{/each}
	</ul>
{/if}
