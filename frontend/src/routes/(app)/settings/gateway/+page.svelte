<script lang="ts">
	import type { GatewayPlatform, CreatePlatformInput, PlatformType } from '$lib/types/gateway';
	import { ApiError } from '$lib/utils/api';
	import { gatewayService } from '$lib/services/gateway';
	import { resolve } from '$app/paths';
	// Platform list state
	let platforms = $state<GatewayPlatform[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	// Add platform form state
	let showAddForm = $state(false);
	let addDisplayName = $state('');
	let addPlatformType = $state<PlatformType>('discord');
	let addSubmitting = $state(false);

	// Delete confirm state
	let deleteConfirmId = $state<string | null>(null);
	let deleteInProgress = $state<string | null>(null);

	// Toggle in-progress
	let toggleInProgress = $state<string | null>(null);

	const platformTypes: { value: PlatformType; label: string }[] = [
		{ value: 'discord', label: 'Discord' },
		{ value: 'telegram', label: 'Telegram' },
		{ value: 'matrix', label: 'Matrix' },
		{ value: 'whatsapp', label: 'WhatsApp' }
	];

	const platformTypeInitial: Record<PlatformType, string> = {
		discord: 'D',
		telegram: 'T',
		matrix: 'M',
		whatsapp: 'W'
	};

	const platformTypeBadgeClass: Record<PlatformType, string> = {
		discord: 'bg-indigo-100 text-indigo-800 dark:bg-indigo-900 dark:text-indigo-200',
		telegram: 'bg-sky-100 text-sky-800 dark:bg-sky-900 dark:text-sky-200',
		matrix: 'bg-purple-100 text-purple-800 dark:bg-purple-900 dark:text-purple-200',
		whatsapp: 'bg-emerald-100 text-emerald-800 dark:bg-emerald-900 dark:text-emerald-200'
	};

	$effect(() => {
		loadPlatforms();
	});

	async function loadPlatforms() {
		loading = true;
		error = null;
		try {
			platforms = await gatewayService.listPlatforms();
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to load platforms';
		} finally {
			loading = false;
		}
	}

	async function addPlatform() {
		if (!addDisplayName.trim()) return;
		addSubmitting = true;
		error = null;
		const input: CreatePlatformInput = {
			platform_type: addPlatformType,
			display_name: addDisplayName.trim()
		};
		try {
			const platform = await gatewayService.createPlatform(input);
			platforms = [...platforms, platform];
			showAddForm = false;
			addDisplayName = '';
			addPlatformType = 'discord';
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to create platform';
		} finally {
			addSubmitting = false;
		}
	}

	async function togglePlatform(platform: GatewayPlatform) {
		toggleInProgress = platform.id;
		error = null;
		try {
			const updated = await gatewayService.updatePlatform(platform.id, {
				is_enabled: !platform.is_enabled
			});
			platforms = platforms.map((p) => (p.id === platform.id ? updated : p));
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to update platform';
		} finally {
			toggleInProgress = null;
		}
	}

	async function deletePlatform(id: string) {
		deleteInProgress = id;
		error = null;
		try {
			await gatewayService.deletePlatform(id);
			platforms = platforms.filter((p) => p.id !== id);
			deleteConfirmId = null;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to delete platform';
		} finally {
			deleteInProgress = null;
		}
	}
</script>

<!-- Header -->
<div class="mb-6 flex items-center justify-between">
	<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">Gateway Platforms</h2>
	<button
		onclick={() => (showAddForm = !showAddForm)}
		class="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
	>
		Add Platform
	</button>
</div>

{#if error}
	<div class="mb-4 rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-950 dark:text-red-300">
		{error}
		<button onclick={() => (error = null)} class="ml-2 font-medium underline hover:no-underline">
			Dismiss
		</button>
	</div>
{/if}

<!-- Add platform form -->
{#if showAddForm}
	<div class="mb-6 rounded-lg border border-zinc-200 p-4 dark:border-zinc-700">
		<h3 class="mb-3 text-sm font-medium text-zinc-900 dark:text-zinc-100">Add Platform</h3>
		<div class="space-y-3">
			<div>
				<label
					for="platform-type"
					class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
				>
					Platform Type
				</label>
				<select
					id="platform-type"
					bind:value={addPlatformType}
					class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
				>
					{#each platformTypes as pt (pt.value)}
						<option value={pt.value}>{pt.label}</option>
					{/each}
				</select>
			</div>
			<div>
				<label
					for="platform-name"
					class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
				>
					Display Name
				</label>
				<input
					id="platform-name"
					type="text"
					bind:value={addDisplayName}
					placeholder="My Discord Server"
					class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
				/>
			</div>
			<div class="flex gap-2">
				<button
					onclick={addPlatform}
					disabled={addSubmitting || !addDisplayName.trim()}
					class="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
				>
					{addSubmitting ? 'Creating...' : 'Create'}
				</button>
				<button
					onclick={() => (showAddForm = false)}
					class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
				>
					Cancel
				</button>
			</div>
		</div>
	</div>
{/if}

<!-- Platform list -->
{#if loading}
	<p class="py-8 text-center text-sm text-zinc-400 dark:text-zinc-500">Loading platforms...</p>
{:else if platforms.length === 0}
	<p class="py-8 text-center text-sm text-zinc-400 dark:text-zinc-500">
		No platforms configured. Add one to get started.
	</p>
{:else}
	<div class="space-y-2">
		{#each platforms as platform (platform.id)}
			<div class="rounded-lg border border-zinc-200 dark:border-zinc-700">
				<div class="flex items-center gap-3 px-4 py-3">
					<!-- Platform type icon -->
					<div
						class={[
							'flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-sm font-bold',
							platformTypeBadgeClass[platform.platform_type]
						]}
					>
						{platformTypeInitial[platform.platform_type]}
					</div>

					<!-- Platform info -->
					<div class="min-w-0 flex-1">
						<div class="flex items-center gap-2">
							<a
								href={resolve('/(app)/settings/gateway/[id]', { id: platform.id })}
								class="text-sm font-medium text-zinc-900 hover:underline dark:text-zinc-100"
							>
								{platform.display_name}
							</a>
							<span
								class={[
									'inline-flex rounded-full px-2 py-0.5 text-xs font-medium',
									platformTypeBadgeClass[platform.platform_type]
								]}
							>
								{platform.platform_type}
							</span>
							<!-- Enabled status dot -->
							<span
								class={[
									'inline-flex h-2 w-2 rounded-full',
									platform.is_enabled ? 'bg-emerald-500' : 'bg-zinc-400 dark:bg-zinc-600'
								]}
								title={platform.is_enabled ? 'Enabled' : 'Disabled'}
							></span>
						</div>
						<p class="mt-0.5 text-xs text-zinc-500 dark:text-zinc-400">
							{platform.is_enabled ? 'Active' : 'Disabled'} &middot; Added {new Date(
								platform.created_at
							).toLocaleDateString()}
						</p>
					</div>

					<!-- Actions -->
					<div class="flex items-center gap-1">
						<button
							onclick={() => togglePlatform(platform)}
							disabled={toggleInProgress === platform.id}
							class="rounded px-2 py-1 text-xs text-zinc-600 hover:bg-zinc-100 disabled:opacity-50 dark:text-zinc-400 dark:hover:bg-zinc-800"
						>
							{platform.is_enabled ? 'Disable' : 'Enable'}
						</button>
						<a
							href={resolve('/(app)/settings/gateway/[id]', { id: platform.id })}
							class="rounded px-2 py-1 text-xs text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800"
						>
							Manage
						</a>
						{#if deleteConfirmId === platform.id}
							<button
								onclick={() => deletePlatform(platform.id)}
								disabled={deleteInProgress === platform.id}
								class="rounded px-2 py-1 text-xs text-red-600 hover:bg-red-50 disabled:opacity-50 dark:text-red-400 dark:hover:bg-red-950"
							>
								Confirm
							</button>
							<button
								onclick={() => (deleteConfirmId = null)}
								class="rounded px-2 py-1 text-xs text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800"
							>
								Cancel
							</button>
						{:else}
							<button
								onclick={() => (deleteConfirmId = platform.id)}
								class="rounded px-2 py-1 text-xs text-red-600 hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-950"
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
