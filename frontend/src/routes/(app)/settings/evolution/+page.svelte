<script lang="ts">
	import type { EvolutionEvent, EvolutionType, AutonomyLevel } from '$lib/types';
	import { ApiError } from '$lib/utils/api';
	import { evolutionService } from '$lib/services/evolution';
	import { resolve } from '$app/paths';

	// --- State ---
	let events = $state<EvolutionEvent[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let configSaving = $state(false);
	let configDirty = $state(false);
	let activeTypeFilter = $state<'all' | EvolutionType>('all');
	let revertConfirmId = $state<string | null>(null);
	let deleteConfirmId = $state<string | null>(null);
	let actionInProgress = $state<string | null>(null);

	// Editable config copy
	let pluginAutonomy = $state<AutonomyLevel>('approval_required');
	let skillAutonomy = $state<AutonomyLevel>('approval_required');
	let instructionAutonomy = $state<AutonomyLevel>('approval_required');
	let automationAutonomy = $state<AutonomyLevel>('approval_required');

	// --- Derived ---
	let pendingEvents = $derived(events.filter((e) => e.status === 'proposed'));

	let activeEvents = $derived.by(() => {
		const base = events.filter((e) => e.status === 'active' || e.status === 'failed');
		if (activeTypeFilter === 'all') return base;
		return base.filter((e) => e.evolution_type === activeTypeFilter);
	});

	let recentEvents = $derived(
		[...events].sort((a, b) => b.updated_at.localeCompare(a.updated_at)).slice(0, 5)
	);

	// --- Load data ---
	$effect(() => {
		loadAll();
	});

	async function loadAll() {
		loading = true;
		error = null;
		try {
			const [evts, cfg] = await Promise.all([
				evolutionService.list(),
				evolutionService.getConfig()
			]);
			events = evts;
			pluginAutonomy = cfg.plugin_autonomy;
			skillAutonomy = cfg.skill_autonomy;
			instructionAutonomy = cfg.instruction_autonomy;
			automationAutonomy = cfg.automation_autonomy;
			configDirty = false;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to load evolution data';
		} finally {
			loading = false;
		}
	}

	// --- Config actions ---
	function markConfigDirty() {
		configDirty = true;
	}

	async function saveConfig() {
		configSaving = true;
		error = null;
		try {
			await evolutionService.updateConfig({
				plugin_autonomy: pluginAutonomy,
				skill_autonomy: skillAutonomy,
				instruction_autonomy: instructionAutonomy,
				automation_autonomy: automationAutonomy
			});
			configDirty = false;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to save configuration';
		} finally {
			configSaving = false;
		}
	}

	// --- Event actions ---
	async function approveEvent(id: string) {
		actionInProgress = id;
		error = null;
		try {
			await evolutionService.update(id, 'approved');
			events = events.filter((e) => e.id !== id);
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to approve proposal';
		} finally {
			actionInProgress = null;
		}
	}

	async function rejectEvent(id: string) {
		actionInProgress = id;
		error = null;
		try {
			await evolutionService.update(id, 'rejected');
			events = events.filter((e) => e.id !== id);
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to reject proposal';
		} finally {
			actionInProgress = null;
		}
	}

	async function revertEvent(id: string) {
		actionInProgress = id;
		error = null;
		try {
			await evolutionService.update(id, 'reverted');
			events = events.map((e) => (e.id === id ? { ...e, status: 'reverted' as const } : e));
			revertConfirmId = null;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to revert evolution';
		} finally {
			actionInProgress = null;
		}
	}

	async function retryEvent(id: string) {
		actionInProgress = id;
		error = null;
		try {
			await evolutionService.update(id, 'approved');
			events = events.map((e) => (e.id === id ? { ...e, status: 'approved' as const } : e));
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to retry evolution';
		} finally {
			actionInProgress = null;
		}
	}

	async function deleteEvent(id: string) {
		actionInProgress = id;
		error = null;
		try {
			await evolutionService.delete(id);
			events = events.filter((e) => e.id !== id);
			deleteConfirmId = null;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to delete evolution';
		} finally {
			actionInProgress = null;
		}
	}

	// --- Helpers ---
	const typeLabel: Record<EvolutionType, string> = {
		plugin: 'Plugin',
		skill: 'Skill',
		instruction: 'Instruction',
		automation: 'Automation'
	};

	const typeBadgeClass: Record<EvolutionType, string> = {
		plugin: 'bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200',
		skill: 'bg-purple-100 text-purple-800 dark:bg-purple-900 dark:text-purple-200',
		instruction: 'bg-amber-100 text-amber-800 dark:bg-amber-900 dark:text-amber-200',
		automation: 'bg-emerald-100 text-emerald-800 dark:bg-emerald-900 dark:text-emerald-200'
	};

	const statusBadgeClass: Record<string, string> = {
		active: 'bg-emerald-100 text-emerald-800 dark:bg-emerald-900 dark:text-emerald-200',
		failed: 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200',
		proposed: 'bg-amber-100 text-amber-800 dark:bg-amber-900 dark:text-amber-200',
		approved: 'bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200',
		executing: 'bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200',
		rejected: 'bg-zinc-100 text-zinc-800 dark:bg-zinc-700 dark:text-zinc-300',
		reverted: 'bg-zinc-100 text-zinc-800 dark:bg-zinc-700 dark:text-zinc-300'
	};

	const autonomyOptions: { value: AutonomyLevel; label: string }[] = [
		{ value: 'auto', label: 'Auto' },
		{ value: 'approval_required', label: 'Approval Required' },
		{ value: 'disabled', label: 'Disabled' }
	];

	const typeFilters: { label: string; value: 'all' | EvolutionType }[] = [
		{ label: 'All', value: 'all' },
		{ label: 'Plugins', value: 'plugin' },
		{ label: 'Skills', value: 'skill' },
		{ label: 'Instructions', value: 'instruction' },
		{ label: 'Automations', value: 'automation' }
	];

	function formatDate(iso: string): string {
		return new Date(iso).toLocaleDateString(undefined, {
			month: 'short',
			day: 'numeric',
			year: 'numeric'
		});
	}

	function formatConfidence(confidence: number): string {
		return `${Math.round(confidence * 100)}%`;
	}
</script>

{#if error}
	<div class="mb-4 rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-950 dark:text-red-300">
		{error}
		<button onclick={() => (error = null)} class="ml-2 font-medium underline hover:no-underline">
			Dismiss
		</button>
	</div>
{/if}

{#if loading}
	<p class="py-8 text-center text-sm text-zinc-400 dark:text-zinc-500">Loading evolution data...</p>
{:else}
	<!-- 1. Autonomy Configuration -->
	<section class="mb-8">
		<h2 class="mb-4 text-sm font-semibold text-zinc-900 dark:text-zinc-100">
			Autonomy Configuration
		</h2>
		<div class="rounded-lg border border-zinc-200 p-4 dark:border-zinc-700">
			<div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
				<div>
					<label
						for="autonomy-plugin"
						class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
					>
						WASM Tools
					</label>
					<select
						id="autonomy-plugin"
						bind:value={pluginAutonomy}
						onchange={markConfigDirty}
						class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
					>
						{#each autonomyOptions as opt (opt.value)}
							<option value={opt.value}>{opt.label}</option>
						{/each}
					</select>
				</div>
				<div>
					<label
						for="autonomy-skill"
						class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
					>
						Skills
					</label>
					<select
						id="autonomy-skill"
						bind:value={skillAutonomy}
						onchange={markConfigDirty}
						class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
					>
						{#each autonomyOptions as opt (opt.value)}
							<option value={opt.value}>{opt.label}</option>
						{/each}
					</select>
				</div>
				<div>
					<label
						for="autonomy-instruction"
						class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
					>
						Instructions
					</label>
					<select
						id="autonomy-instruction"
						bind:value={instructionAutonomy}
						onchange={markConfigDirty}
						class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
					>
						{#each autonomyOptions as opt (opt.value)}
							<option value={opt.value}>{opt.label}</option>
						{/each}
					</select>
				</div>
				<div>
					<label
						for="autonomy-automation"
						class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
					>
						Automations
					</label>
					<select
						id="autonomy-automation"
						bind:value={automationAutonomy}
						onchange={markConfigDirty}
						class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
					>
						{#each autonomyOptions as opt (opt.value)}
							<option value={opt.value}>{opt.label}</option>
						{/each}
					</select>
				</div>
			</div>
			<div class="mt-4">
				<button
					onclick={saveConfig}
					disabled={configSaving || !configDirty}
					class="rounded-md bg-zinc-900 px-4 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
				>
					{configSaving ? 'Saving...' : 'Save'}
				</button>
			</div>
		</div>
	</section>

	<!-- 2. Pending Proposals -->
	{#if pendingEvents.length > 0}
		<section class="mb-8">
			<h2 class="mb-4 text-sm font-semibold text-zinc-900 dark:text-zinc-100">
				Pending Proposals
				<span
					class="ml-2 inline-flex items-center rounded-full bg-amber-100 px-2 py-0.5 text-xs font-medium text-amber-800 dark:bg-amber-900 dark:text-amber-200"
				>
					{pendingEvents.length}
				</span>
			</h2>
			<div class="space-y-3">
				{#each pendingEvents as event (event.id)}
					<div
						class="rounded-lg border border-amber-200 bg-amber-50/50 p-4 dark:border-amber-900 dark:bg-amber-950/30"
					>
						<div class="flex items-start justify-between gap-4">
							<div class="min-w-0 flex-1">
								<div class="flex items-center gap-2">
									<span class="text-sm font-medium text-zinc-900 dark:text-zinc-100">
										{event.title}
									</span>
									<span
										class={[
											'inline-flex rounded-full px-2 py-0.5 text-xs font-medium',
											typeBadgeClass[event.evolution_type]
										]}
									>
										{typeLabel[event.evolution_type]}
									</span>
									<span class="text-xs text-zinc-500 dark:text-zinc-400">
										{formatConfidence(event.confidence)} confidence
									</span>
								</div>
								<p class="mt-1 text-sm text-zinc-600 dark:text-zinc-400">
									{event.description}
								</p>
								{#if event.payload.evidence}
									<p class="mt-2 text-xs text-zinc-500 italic dark:text-zinc-500">
										Evidence: {event.payload.evidence}
									</p>
								{/if}
							</div>
							<div class="flex shrink-0 gap-2">
								<button
									onclick={() => approveEvent(event.id)}
									disabled={actionInProgress === event.id}
									class="rounded-md bg-emerald-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-emerald-500 disabled:opacity-50 dark:bg-emerald-700 dark:hover:bg-emerald-600"
								>
									Approve
								</button>
								<button
									onclick={() => rejectEvent(event.id)}
									disabled={actionInProgress === event.id}
									class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
								>
									Reject
								</button>
								<button
									onclick={() => deleteEvent(event.id)}
									disabled={actionInProgress === event.id}
									class="rounded-md border border-red-300 px-3 py-1.5 text-sm text-red-700 hover:bg-red-50 disabled:opacity-50 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-950"
								>
									Delete
								</button>
							</div>
						</div>
					</div>
				{/each}
			</div>
		</section>
	{/if}

	<!-- 3. Active Evolutions -->
	<section class="mb-8">
		<h2 class="mb-4 text-sm font-semibold text-zinc-900 dark:text-zinc-100">Active Evolutions</h2>

		<!-- Filter bar -->
		<div class="mb-4 flex gap-1 rounded-lg bg-zinc-100 p-1 dark:bg-zinc-800">
			{#each typeFilters as filter (filter.value)}
				<button
					onclick={() => (activeTypeFilter = filter.value)}
					class={[
						'rounded-md px-3 py-1.5 text-sm font-medium transition-colors',
						activeTypeFilter === filter.value
							? 'bg-white text-zinc-900 shadow-sm dark:bg-zinc-700 dark:text-zinc-100'
							: 'text-zinc-600 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-zinc-200'
					]}
				>
					{filter.label}
				</button>
			{/each}
		</div>

		{#if activeEvents.length === 0}
			<p class="py-6 text-center text-sm text-zinc-400 dark:text-zinc-500">
				No active evolutions{activeTypeFilter !== 'all'
					? ` for ${typeLabel[activeTypeFilter]}s`
					: ''}.
			</p>
		{:else}
			<div class="space-y-2">
				{#each activeEvents as event (event.id)}
					<div class="rounded-lg border border-zinc-200 px-4 py-3 dark:border-zinc-700">
						<div class="flex items-start justify-between gap-4">
							<div class="min-w-0 flex-1">
								<div class="flex items-center gap-2">
									<span class="text-sm font-medium text-zinc-900 dark:text-zinc-100">
										{event.title}
									</span>
									<span
										class={[
											'inline-flex rounded-full px-2 py-0.5 text-xs font-medium',
											typeBadgeClass[event.evolution_type]
										]}
									>
										{typeLabel[event.evolution_type]}
									</span>
									<span
										class={[
											'inline-flex rounded-full px-2 py-0.5 text-xs font-medium',
											statusBadgeClass[event.status]
										]}
									>
										{event.status}
									</span>
									<span class="text-xs text-zinc-500 dark:text-zinc-400">
										{formatConfidence(event.confidence)}
									</span>
								</div>
								<p class="mt-1 text-sm text-zinc-600 dark:text-zinc-400">
									{event.description}
								</p>
								<div class="mt-2 flex items-center gap-3 text-xs text-zinc-400 dark:text-zinc-500">
									<span>{formatDate(event.created_at)}</span>
									{#if event.result}
										{#if event.result.usage_count !== undefined}
											<span>{event.result.usage_count} uses</span>
										{/if}
										{#if event.result.success_rate !== undefined}
											<span>{Math.round(Number(event.result.success_rate) * 100)}% success</span>
										{/if}
									{/if}
								</div>
								{#if event.status === 'failed' && event.result?.error}
									<p
										class="mt-2 rounded bg-red-50 p-2 text-xs text-red-700 dark:bg-red-950/50 dark:text-red-300"
									>
										{event.result.error}
									</p>
								{/if}
							</div>
							<div class="flex shrink-0 gap-2">
								{#if event.status === 'failed'}
									<button
										onclick={() => retryEvent(event.id)}
										disabled={actionInProgress === event.id}
										class="rounded-md border border-amber-300 px-3 py-1.5 text-sm text-amber-700 hover:bg-amber-50 disabled:opacity-50 dark:border-amber-700 dark:text-amber-300 dark:hover:bg-amber-950"
									>
										Retry
									</button>
								{/if}
								{#if event.status === 'active'}
									{#if revertConfirmId === event.id}
										<button
											onclick={() => revertEvent(event.id)}
											disabled={actionInProgress === event.id}
											class="rounded-md bg-red-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-red-500 disabled:opacity-50"
										>
											Confirm Revert
										</button>
										<button
											onclick={() => (revertConfirmId = null)}
											class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
										>
											Cancel
										</button>
									{:else}
										<button
											onclick={() => (revertConfirmId = event.id)}
											class="rounded-md border border-red-300 px-3 py-1.5 text-sm text-red-700 hover:bg-red-50 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-950"
										>
											Revert
										</button>
									{/if}
								{/if}
								{#if deleteConfirmId === event.id}
									<button
										onclick={() => deleteEvent(event.id)}
										disabled={actionInProgress === event.id}
										class="rounded-md bg-red-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-red-500 disabled:opacity-50"
									>
										Confirm Delete
									</button>
									<button
										onclick={() => (deleteConfirmId = null)}
										class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
									>
										Cancel
									</button>
								{:else if event.status !== 'approved' && event.status !== 'executing'}
									<button
										onclick={() => (deleteConfirmId = event.id)}
										class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-500 hover:border-red-300 hover:bg-red-50 hover:text-red-700 disabled:opacity-50 dark:border-zinc-700 dark:text-zinc-400 dark:hover:border-red-800 dark:hover:bg-red-950 dark:hover:text-red-400"
									>
										Delete
									</button>
								{/if}
							</div>
						</div>
					</div>
				{/each}
			</div>
		{/if}
	</section>

	<!-- 4. Compact Timeline -->
	{#if recentEvents.length > 0}
		<section>
			<div class="mb-3 flex items-center justify-between">
				<h2 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">Recent Activity</h2>
				<a
					href={resolve('/(app)/settings/evolution/timeline')}
					class="text-xs font-medium text-zinc-600 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-zinc-200"
				>
					View all
				</a>
			</div>
			<div class="rounded-lg border border-zinc-200 dark:border-zinc-700">
				{#each recentEvents as event, i (event.id)}
					<div
						class={[
							'flex items-center gap-3 px-4 py-2.5',
							i > 0 && 'border-t border-zinc-100 dark:border-zinc-800'
						]}
					>
						<span
							class={[
								'inline-flex shrink-0 rounded-full px-2 py-0.5 text-xs font-medium',
								statusBadgeClass[event.status] ??
									'bg-zinc-100 text-zinc-800 dark:bg-zinc-700 dark:text-zinc-300'
							]}
						>
							{event.status}
						</span>
						<span class="min-w-0 flex-1 truncate text-sm text-zinc-700 dark:text-zinc-300">
							{event.title}
						</span>
						<span
							class={[
								'shrink-0 rounded-full px-1.5 py-0.5 text-xs',
								typeBadgeClass[event.evolution_type]
							]}
						>
							{typeLabel[event.evolution_type]}
						</span>
						<span class="shrink-0 text-xs text-zinc-400 dark:text-zinc-500">
							{formatDate(event.updated_at)}
						</span>
					</div>
				{/each}
			</div>
		</section>
	{/if}
{/if}
