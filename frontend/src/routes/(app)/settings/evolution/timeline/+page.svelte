<script lang="ts">
	import type { EvolutionEvent, EvolutionType, EvolutionStatus } from '$lib/types';
	import { ApiError } from '$lib/utils/api';
	import { evolutionService } from '$lib/services/evolution';
	import { formatRelativeTime } from '$lib/utils/time';

	type TypeFilter = 'all' | EvolutionType;
	type StatusFilter = 'all' | EvolutionStatus;
	type TimeRange = '24h' | '7d' | '30d' | 'all';

	const PAGE_SIZE = 20;

	// Filter state
	let typeFilter = $state<TypeFilter>('all');
	let statusFilter = $state<StatusFilter>('all');
	let timeRange = $state<TimeRange>('all');

	// Data state
	let events = $state<EvolutionEvent[]>([]);
	let loading = $state(true);
	let loadingMore = $state(false);
	let error = $state<string | null>(null);
	let hasMore = $state(false);
	let actionLoading = $state<string | null>(null);
	let deleteConfirmId = $state<string | null>(null);

	const typeOptions: { label: string; value: TypeFilter }[] = [
		{ label: 'All', value: 'all' },
		{ label: 'Plugin', value: 'plugin' },
		{ label: 'Skill', value: 'skill' },
		{ label: 'Instruction', value: 'instruction' },
		{ label: 'Automation', value: 'automation' }
	];

	const statusOptions: { label: string; value: StatusFilter }[] = [
		{ label: 'All', value: 'all' },
		{ label: 'Proposed', value: 'proposed' },
		{ label: 'Active', value: 'active' },
		{ label: 'Failed', value: 'failed' },
		{ label: 'Rejected', value: 'rejected' },
		{ label: 'Reverted', value: 'reverted' }
	];

	const timeRangeOptions: { label: string; value: TimeRange }[] = [
		{ label: 'Last 24h', value: '24h' },
		{ label: 'Last 7 days', value: '7d' },
		{ label: 'Last 30 days', value: '30d' },
		{ label: 'All time', value: 'all' }
	];

	const typeBadgeClass: Record<EvolutionType, string> = {
		plugin: 'bg-blue-100 text-blue-800 dark:bg-blue-900/50 dark:text-blue-300',
		skill: 'bg-purple-100 text-purple-800 dark:bg-purple-900/50 dark:text-purple-300',
		instruction: 'bg-teal-100 text-teal-800 dark:bg-teal-900/50 dark:text-teal-300',
		automation: 'bg-orange-100 text-orange-800 dark:bg-orange-900/50 dark:text-orange-300'
	};

	const statusBadgeClass: Record<string, string> = {
		proposed: 'bg-amber-100 text-amber-800 dark:bg-amber-900/50 dark:text-amber-300',
		approved: 'bg-sky-100 text-sky-800 dark:bg-sky-900/50 dark:text-sky-300',
		executing: 'bg-sky-100 text-sky-800 dark:bg-sky-900/50 dark:text-sky-300',
		active: 'bg-emerald-100 text-emerald-800 dark:bg-emerald-900/50 dark:text-emerald-300',
		failed: 'bg-red-100 text-red-800 dark:bg-red-900/50 dark:text-red-300',
		rejected: 'bg-zinc-100 text-zinc-800 dark:bg-zinc-700 dark:text-zinc-300',
		reverted: 'bg-zinc-100 text-zinc-800 dark:bg-zinc-700 dark:text-zinc-300'
	};

	const statusDotClass: Record<string, string> = {
		proposed: 'bg-amber-500',
		approved: 'bg-sky-500',
		executing: 'bg-sky-500',
		active: 'bg-emerald-500',
		failed: 'bg-red-500',
		rejected: 'bg-zinc-400 dark:bg-zinc-500',
		reverted: 'bg-zinc-400 dark:bg-zinc-500'
	};

	let filteredByTime = $derived.by(() => {
		if (timeRange === 'all') return events;
		const now = Date.now();
		const cutoffs: Record<string, number> = {
			'24h': 24 * 60 * 60 * 1000,
			'7d': 7 * 24 * 60 * 60 * 1000,
			'30d': 30 * 24 * 60 * 60 * 1000
		};
		const cutoff = now - (cutoffs[timeRange] ?? 0);
		return events.filter((e) => new Date(e.created_at).getTime() >= cutoff);
	});

	// Load data when type/status filters change
	$effect(() => {
		// Read reactive dependencies
		const t = typeFilter;
		const s = statusFilter;

		loadEvents(t, s);
	});

	async function loadEvents(type: TypeFilter, status: StatusFilter) {
		loading = true;
		error = null;
		try {
			const data = await evolutionService.timeline(
				PAGE_SIZE + 1,
				type === 'all' ? undefined : type,
				status === 'all' ? undefined : status
			);
			hasMore = data.length > PAGE_SIZE;
			events = data.slice(0, PAGE_SIZE);
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to load timeline';
		} finally {
			loading = false;
		}
	}

	async function loadMore() {
		loadingMore = true;
		try {
			const data = await evolutionService.timeline(
				events.length + PAGE_SIZE + 1,
				typeFilter === 'all' ? undefined : typeFilter,
				statusFilter === 'all' ? undefined : statusFilter
			);
			hasMore = data.length > events.length + PAGE_SIZE;
			events = data.slice(0, events.length + PAGE_SIZE);
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to load more';
		} finally {
			loadingMore = false;
		}
	}

	async function updateStatus(id: string, status: string) {
		actionLoading = id;
		error = null;
		try {
			const updated = await evolutionService.update(id, status);
			events = events.map((e) => (e.id === id ? updated : e));
		} catch (err) {
			error = err instanceof ApiError ? err.message : `Failed to ${status} evolution`;
		} finally {
			actionLoading = null;
		}
	}

	async function deleteEvolution(id: string) {
		actionLoading = id;
		error = null;
		try {
			await evolutionService.delete(id);
			events = events.filter((e) => e.id !== id);
			deleteConfirmId = null;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to delete evolution';
		} finally {
			actionLoading = null;
		}
	}

	function formatTimestamp(dateStr: string): string {
		return new Date(dateStr).toLocaleString(undefined, {
			month: 'short',
			day: 'numeric',
			hour: '2-digit',
			minute: '2-digit'
		});
	}
</script>

<!-- Filters -->
<div class="mb-6 flex flex-wrap items-center gap-3">
	<div>
		<label for="type-filter" class="mb-1 block text-xs font-medium text-zinc-500 dark:text-zinc-400"
			>Type</label
		>
		<select
			id="type-filter"
			bind:value={typeFilter}
			class="rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
		>
			{#each typeOptions as opt (opt.value)}
				<option value={opt.value}>{opt.label}</option>
			{/each}
		</select>
	</div>

	<div>
		<label
			for="status-filter"
			class="mb-1 block text-xs font-medium text-zinc-500 dark:text-zinc-400">Status</label
		>
		<select
			id="status-filter"
			bind:value={statusFilter}
			class="rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
		>
			{#each statusOptions as opt (opt.value)}
				<option value={opt.value}>{opt.label}</option>
			{/each}
		</select>
	</div>

	<div>
		<label for="time-filter" class="mb-1 block text-xs font-medium text-zinc-500 dark:text-zinc-400"
			>Time range</label
		>
		<select
			id="time-filter"
			bind:value={timeRange}
			class="rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
		>
			{#each timeRangeOptions as opt (opt.value)}
				<option value={opt.value}>{opt.label}</option>
			{/each}
		</select>
	</div>
</div>

{#if error}
	<div class="mb-4 rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-950 dark:text-red-300">
		{error}
		<button onclick={() => (error = null)} class="ml-2 font-medium underline hover:no-underline">
			Dismiss
		</button>
	</div>
{/if}

{#if loading}
	<p class="py-12 text-center text-sm text-zinc-400 dark:text-zinc-500">Loading timeline...</p>
{:else if filteredByTime.length === 0}
	<p class="py-12 text-center text-sm text-zinc-400 dark:text-zinc-500">
		{events.length === 0 ? 'No evolution events yet.' : 'No events match the selected filters.'}
	</p>
{:else}
	<!-- Timeline -->
	<div class="relative ml-4">
		<!-- Vertical line -->
		<div
			class="absolute top-0 bottom-0 left-3 w-px bg-zinc-200 dark:bg-zinc-700"
			aria-hidden="true"
		></div>

		<div class="space-y-6">
			{#each filteredByTime as event (event.id)}
				{@const isActioning = actionLoading === event.id}
				<div class="relative pl-10">
					<!-- Timeline dot -->
					<div
						class={[
							'absolute left-1.5 top-1.5 h-3 w-3 rounded-full ring-2 ring-white dark:ring-zinc-900',
							statusDotClass[event.status] ?? 'bg-zinc-400'
						]}
						aria-hidden="true"
					></div>

					<!-- Event card -->
					<div class="rounded-lg border border-zinc-200 dark:border-zinc-700">
						<!-- Header -->
						<div class="px-4 py-3">
							<div class="flex items-start justify-between gap-2">
								<div class="min-w-0 flex-1">
									<div class="flex flex-wrap items-center gap-2">
										<span
											class="text-xs text-zinc-500 dark:text-zinc-400"
											title={new Date(event.created_at).toLocaleString()}
										>
											{formatRelativeTime(event.created_at)}
										</span>
										<span
											class={[
												'inline-flex rounded-full px-2 py-0.5 text-xs font-medium',
												typeBadgeClass[event.evolution_type]
											]}
										>
											{event.evolution_type}
										</span>
										<span
											class={[
												'inline-flex rounded-full px-2 py-0.5 text-xs font-medium',
												statusBadgeClass[event.status] ?? statusBadgeClass.rejected
											]}
										>
											{event.status}
										</span>
									</div>
									<h3 class="mt-1 text-sm font-medium text-zinc-900 dark:text-zinc-100">
										{event.title}
									</h3>
								</div>
							</div>

							<!-- Summary -->
							<p class="mt-1 text-xs text-zinc-600 dark:text-zinc-400">
								{event.description}
							</p>
							<div class="mt-1.5 flex items-center gap-3 text-xs text-zinc-500 dark:text-zinc-400">
								<span>Confidence: {Math.round(event.confidence * 100)}%</span>
								<span>{event.source_count} source{event.source_count !== 1 ? 's' : ''}</span>
							</div>
						</div>

						<!-- Status history branch -->
						{#if event.status_history.length > 0}
							<div
								class="border-t border-zinc-100 bg-zinc-50/50 px-4 py-3 dark:border-zinc-800 dark:bg-zinc-800/30"
							>
								<p class="mb-2 text-xs font-medium text-zinc-500 dark:text-zinc-400">
									Status history
								</p>
								<div class="relative ml-2">
									<!-- Branch line -->
									<div
										class="absolute top-1 bottom-1 left-1 w-px bg-zinc-200 dark:bg-zinc-700"
										aria-hidden="true"
									></div>

									<div class="space-y-2">
										{#each event.status_history as entry, i (i)}
											<div class="relative flex items-start gap-3 pl-5">
												<!-- Branch dot -->
												<div
													class={[
														'absolute left-0 top-1 h-2.5 w-2.5 rounded-full ring-2 ring-zinc-50 dark:ring-zinc-800',
														statusDotClass[entry.status] ?? 'bg-zinc-400'
													]}
													aria-hidden="true"
												></div>
												<div class="min-w-0 flex-1">
													<div class="flex items-center gap-2">
														<span
															class={[
																'inline-flex rounded-full px-1.5 py-0.5 text-xs font-medium',
																statusBadgeClass[entry.status] ?? statusBadgeClass.rejected
															]}
														>
															{entry.status}
														</span>
														<span class="text-xs text-zinc-400 dark:text-zinc-500">
															{formatTimestamp(entry.at)}
														</span>
														{#if entry.by}
															<span class="text-xs text-zinc-400 dark:text-zinc-500">
																by {entry.by}
															</span>
														{/if}
													</div>
													<!-- Show error for failed status -->
													{#if entry.status === 'failed' && event.result?.error}
														<p class="mt-0.5 text-xs text-red-600 dark:text-red-400">
															{event.result.error}
														</p>
													{/if}
													<!-- Show usage metrics for active status -->
													{#if entry.status === 'active' && event.result?.usage_count != null}
														<p class="mt-0.5 text-xs text-emerald-600 dark:text-emerald-400">
															{event.result.usage_count} usage{event.result.usage_count !== 1
																? 's'
																: ''}
														</p>
													{/if}
												</div>
											</div>
										{/each}
									</div>
								</div>
							</div>
						{/if}

						<!-- Actions -->
						{#if event.status === 'proposed' || event.status === 'active' || event.status === 'failed' || event.status === 'rejected' || event.status === 'reverted'}
							<div
								class="flex items-center gap-2 border-t border-zinc-100 px-4 py-2 dark:border-zinc-800"
							>
								{#if event.status === 'proposed'}
									<button
										onclick={() => updateStatus(event.id, 'approved')}
										disabled={isActioning}
										class="rounded px-2.5 py-1 text-xs font-medium text-emerald-700 hover:bg-emerald-50 disabled:opacity-50 dark:text-emerald-400 dark:hover:bg-emerald-950"
									>
										{isActioning ? 'Approving...' : 'Approve'}
									</button>
									<button
										onclick={() => updateStatus(event.id, 'rejected')}
										disabled={isActioning}
										class="rounded px-2.5 py-1 text-xs font-medium text-red-700 hover:bg-red-50 disabled:opacity-50 dark:text-red-400 dark:hover:bg-red-950"
									>
										Reject
									</button>
								{:else if event.status === 'active'}
									<button
										onclick={() => updateStatus(event.id, 'reverted')}
										disabled={isActioning}
										class="rounded px-2.5 py-1 text-xs font-medium text-amber-700 hover:bg-amber-50 disabled:opacity-50 dark:text-amber-400 dark:hover:bg-amber-950"
									>
										{isActioning ? 'Reverting...' : 'Revert'}
									</button>
								{:else if event.status === 'failed'}
									<button
										onclick={() => updateStatus(event.id, 'approved')}
										disabled={isActioning}
										class="rounded px-2.5 py-1 text-xs font-medium text-sky-700 hover:bg-sky-50 disabled:opacity-50 dark:text-sky-400 dark:hover:bg-sky-950"
									>
										{isActioning ? 'Retrying...' : 'Retry'}
									</button>
								{/if}
								<!-- Delete (available for all deletable statuses) -->
								{#if deleteConfirmId === event.id}
									<button
										onclick={() => deleteEvolution(event.id)}
										disabled={isActioning}
										class="rounded px-2.5 py-1 text-xs font-medium bg-red-600 text-white hover:bg-red-500 disabled:opacity-50"
									>
										Confirm Delete
									</button>
									<button
										onclick={() => (deleteConfirmId = null)}
										class="rounded px-2.5 py-1 text-xs font-medium text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800"
									>
										Cancel
									</button>
								{:else}
									<button
										onclick={() => (deleteConfirmId = event.id)}
										disabled={isActioning}
										class="rounded px-2.5 py-1 text-xs font-medium text-zinc-500 hover:bg-red-50 hover:text-red-700 disabled:opacity-50 dark:text-zinc-400 dark:hover:bg-red-950 dark:hover:text-red-400"
									>
										Delete
									</button>
								{/if}
							</div>
						{/if}
					</div>
				</div>
			{/each}
		</div>
	</div>

	<!-- Load more -->
	{#if hasMore}
		<div class="mt-6 text-center">
			<button
				onclick={loadMore}
				disabled={loadingMore}
				class="rounded-md border border-zinc-300 px-4 py-2 text-sm text-zinc-700 hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
			>
				{loadingMore ? 'Loading...' : 'Load more'}
			</button>
		</div>
	{/if}
{/if}
