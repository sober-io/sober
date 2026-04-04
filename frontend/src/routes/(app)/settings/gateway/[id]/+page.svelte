<script lang="ts">
	import type {
		GatewayPlatform,
		GatewayChannelMapping,
		GatewayUserMapping,
		ExternalChannel,
		CreateMappingInput,
		CreateUserMappingInput
	} from '$lib/types/gateway';
	import { ApiError } from '$lib/utils/api';
	import { gatewayService } from '$lib/services/gateway';
	import { page } from '$app/stores';
	import { resolve } from '$app/paths';

	const platformId = $derived($page.params.id ?? '');

	// Data state
	let platform = $state<GatewayPlatform | null>(null);
	let mappings = $state<GatewayChannelMapping[]>([]);
	let userMappings = $state<GatewayUserMapping[]>([]);
	let externalChannels = $state<ExternalChannel[]>([]);
	let channelsAvailable = $state(true);

	// Loading / error state
	let loading = $state(true);
	let error = $state<string | null>(null);

	// Channel mapping form state
	let showAddMappingForm = $state(false);
	let mappingChannelId = $state('');
	let mappingChannelName = $state('');
	let mappingConversationId = $state('');
	let mappingSubmitting = $state(false);
	let useManualChannel = $state(false);

	// User mapping form state
	let showAddUserForm = $state(false);
	let userExternalId = $state('');
	let userExternalUsername = $state('');
	let userSoberId = $state('');
	let userSubmitting = $state(false);

	// Credentials form state
	let showCredentials = $state(false);
	let credentialFields = $state<Record<string, string>>({});
	let credentialsSubmitting = $state(false);
	let credentialsSaved = $state(false);

	interface CredentialField {
		key: string;
		label: string;
		type: string;
	}

	const credentialSchema: Record<string, CredentialField[]> = {
		discord: [{ key: 'bot_token', label: 'Bot Token', type: 'password' }],
		telegram: [{ key: 'bot_token', label: 'Bot Token', type: 'password' }],
		matrix: [
			{ key: 'homeserver_url', label: 'Homeserver URL', type: 'text' },
			{ key: 'access_token', label: 'Access Token', type: 'password' }
		],
		whatsapp: [
			{ key: 'phone_number_id', label: 'Phone Number ID', type: 'text' },
			{ key: 'access_token', label: 'Access Token', type: 'password' }
		]
	};

	function initCredentialFields() {
		if (!platform) return;
		const schema = credentialSchema[platform.platform_type] ?? [];
		const fields: Record<string, string> = {};
		for (const f of schema) {
			fields[f.key] = '';
		}
		credentialFields = fields;
	}

	async function saveCredentials() {
		credentialsSubmitting = true;
		credentialsSaved = false;
		error = null;
		try {
			await gatewayService.storeCredentials(platformId, credentialFields);
			credentialsSaved = true;
			showCredentials = false;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to save credentials';
		} finally {
			credentialsSubmitting = false;
		}
	}

	// Delete confirm state
	let deleteMappingConfirmId = $state<string | null>(null);
	let deleteMappingInProgress = $state<string | null>(null);
	let deleteUserConfirmId = $state<string | null>(null);
	let deleteUserInProgress = $state<string | null>(null);

	$effect(() => {
		const id = platformId;
		loadAll(id);
	});

	async function loadAll(id: string) {
		loading = true;
		error = null;
		try {
			const [p, m, u] = await Promise.all([
				gatewayService.getPlatform(id),
				gatewayService.listMappings(id),
				gatewayService.listUserMappings(id)
			]);
			platform = p;
			mappings = m;
			userMappings = u;

			// Attempt to load channels from the gateway (may not be available)
			try {
				externalChannels = await gatewayService.listChannels(id);
				channelsAvailable = true;
			} catch {
				externalChannels = [];
				channelsAvailable = false;
			}
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to load platform data';
		} finally {
			loading = false;
		}
	}

	function onChannelSelect(e: Event) {
		const select = e.currentTarget as HTMLSelectElement;
		const channelId = select.value;
		if (!channelId) {
			mappingChannelId = '';
			mappingChannelName = '';
			return;
		}
		const channel = externalChannels.find((c) => c.id === channelId);
		if (channel) {
			mappingChannelId = channel.id;
			mappingChannelName = channel.name;
		}
	}

	async function addMapping() {
		if (!mappingChannelId.trim() || !mappingConversationId.trim()) return;
		mappingSubmitting = true;
		error = null;
		const input: CreateMappingInput = {
			external_channel_id: mappingChannelId.trim(),
			external_channel_name: mappingChannelName.trim() || mappingChannelId.trim(),
			conversation_id: mappingConversationId.trim()
		};
		try {
			const mapping = await gatewayService.createMapping(platformId, input);
			mappings = [...mappings, mapping];
			showAddMappingForm = false;
			mappingChannelId = '';
			mappingChannelName = '';
			mappingConversationId = '';
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to create channel mapping';
		} finally {
			mappingSubmitting = false;
		}
	}

	async function deleteMapping(id: string) {
		deleteMappingInProgress = id;
		error = null;
		try {
			await gatewayService.deleteMapping(id);
			mappings = mappings.filter((m) => m.id !== id);
			deleteMappingConfirmId = null;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to delete mapping';
		} finally {
			deleteMappingInProgress = null;
		}
	}

	async function addUserMapping() {
		if (!userExternalId.trim() || !userSoberId.trim()) return;
		userSubmitting = true;
		error = null;
		const input: CreateUserMappingInput = {
			external_user_id: userExternalId.trim(),
			external_username: userExternalUsername.trim() || userExternalId.trim(),
			user_id: userSoberId.trim()
		};
		try {
			const mapping = await gatewayService.createUserMapping(platformId, input);
			userMappings = [...userMappings, mapping];
			showAddUserForm = false;
			userExternalId = '';
			userExternalUsername = '';
			userSoberId = '';
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to create user mapping';
		} finally {
			userSubmitting = false;
		}
	}

	async function deleteUserMapping(id: string) {
		deleteUserInProgress = id;
		error = null;
		try {
			await gatewayService.deleteUserMapping(id);
			userMappings = userMappings.filter((m) => m.id !== id);
			deleteUserConfirmId = null;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to delete user mapping';
		} finally {
			deleteUserInProgress = null;
		}
	}
</script>

<!-- Breadcrumb -->
<div class="mb-6 flex items-center gap-2 text-sm text-zinc-500 dark:text-zinc-400">
	<a href={resolve('/(app)/settings/gateway')} class="hover:text-zinc-900 dark:hover:text-zinc-100">
		Gateway
	</a>
	<span>/</span>
	<span class="text-zinc-900 dark:text-zinc-100">
		{loading ? 'Loading...' : (platform?.display_name ?? 'Platform')}
	</span>
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
	<p class="py-8 text-center text-sm text-zinc-400 dark:text-zinc-500">Loading...</p>
{:else if platform}
	<!-- Platform info header -->
	<div class="mb-8 flex items-center gap-3">
		<div>
			<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">
				{platform.display_name}
			</h2>
			<p class="text-sm text-zinc-500 dark:text-zinc-400">
				{platform.platform_type} &middot;
				<span class={platform.is_enabled ? 'text-emerald-600 dark:text-emerald-400' : ''}>
					{platform.is_enabled ? 'Enabled' : 'Disabled'}
				</span>
			</p>
		</div>
	</div>

	<!-- Credentials section -->
	<section class="mb-8">
		<div class="mb-4 flex items-center justify-between">
			<h3 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">Credentials</h3>
			<div class="flex items-center gap-2">
				{#if credentialsSaved}
					<span class="text-xs text-emerald-600 dark:text-emerald-400">Saved</span>
				{/if}
				<button
					onclick={() => {
						showCredentials = !showCredentials;
						if (showCredentials) initCredentialFields();
					}}
					class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
				>
					{showCredentials ? 'Cancel' : 'Update Credentials'}
				</button>
			</div>
		</div>

		{#if showCredentials}
			<div class="rounded-lg border border-zinc-200 p-4 dark:border-zinc-700">
				<div class="space-y-3">
					{#each credentialSchema[platform.platform_type] ?? [] as field (field.key)}
						<div>
							<label
								for={`cred-${field.key}`}
								class="mb-1 block text-xs font-medium text-zinc-700 dark:text-zinc-300"
							>
								{field.label}
							</label>
							<input
								id={`cred-${field.key}`}
								type={field.type}
								bind:value={credentialFields[field.key]}
								placeholder={field.label}
								class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-100"
							/>
						</div>
					{/each}
					<div class="flex justify-end">
						<button
							onclick={saveCredentials}
							disabled={credentialsSubmitting}
							class="rounded-md bg-zinc-900 px-4 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
						>
							{credentialsSubmitting ? 'Saving...' : 'Save Credentials'}
						</button>
					</div>
				</div>
				<p class="mt-3 text-xs text-zinc-500 dark:text-zinc-400">
					After saving, the gateway will pick up the new credentials on next reload.
				</p>
			</div>
		{:else}
			<p class="text-sm text-zinc-500 dark:text-zinc-400">
				Credentials are stored but not displayed for security.
			</p>
		{/if}
	</section>

	<!-- Channel Mappings section -->
	<section class="mb-8">
		<div class="mb-4 flex items-center justify-between">
			<h3 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">Channel Mappings</h3>
			<button
				onclick={() => (showAddMappingForm = !showAddMappingForm)}
				class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
			>
				Add Mapping
			</button>
		</div>

		{#if showAddMappingForm}
			<div class="mb-4 rounded-lg border border-zinc-200 p-4 dark:border-zinc-700">
				<h4 class="mb-3 text-sm font-medium text-zinc-900 dark:text-zinc-100">
					New Channel Mapping
				</h4>
				<div class="space-y-3">
					{#if channelsAvailable && externalChannels.length > 0 && !useManualChannel}
						<div>
							<label
								for="mapping-channel-select"
								class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
							>
								Channel
							</label>
							<select
								id="mapping-channel-select"
								onchange={onChannelSelect}
								class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
							>
								<option value="">Select a channel...</option>
								{#each externalChannels as channel (channel.id)}
									<option value={channel.id}>{channel.name} ({channel.kind})</option>
								{/each}
							</select>
							<button
								onclick={() => (useManualChannel = true)}
								class="mt-1 text-xs text-zinc-500 hover:text-zinc-700 dark:text-zinc-400 dark:hover:text-zinc-200"
							>
								Enter channel ID manually instead
							</button>
						</div>
					{:else}
						<div>
							<label
								for="mapping-channel-id"
								class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
							>
								External Channel ID
							</label>
							<input
								id="mapping-channel-id"
								type="text"
								bind:value={mappingChannelId}
								placeholder="123456789"
								class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm font-mono text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
							/>
						</div>
						<div>
							<label
								for="mapping-channel-name"
								class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
							>
								Channel Name (optional)
							</label>
							<input
								id="mapping-channel-name"
								type="text"
								bind:value={mappingChannelName}
								placeholder="#general"
								class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
							/>
						</div>
						{#if channelsAvailable && externalChannels.length > 0}
							<button
								onclick={() => (useManualChannel = false)}
								class="text-xs text-zinc-500 hover:text-zinc-700 dark:text-zinc-400 dark:hover:text-zinc-200"
							>
								Select from available channels instead
							</button>
						{/if}
					{/if}

					<div>
						<label
							for="mapping-conversation-id"
							class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
						>
							Conversation ID
						</label>
						<input
							id="mapping-conversation-id"
							type="text"
							bind:value={mappingConversationId}
							placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
							class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm font-mono text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
						/>
					</div>

					<div class="flex gap-2">
						<button
							onclick={addMapping}
							disabled={mappingSubmitting ||
								!mappingChannelId.trim() ||
								!mappingConversationId.trim()}
							class="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
						>
							{mappingSubmitting ? 'Adding...' : 'Add'}
						</button>
						<button
							onclick={() => {
								showAddMappingForm = false;
								useManualChannel = false;
								mappingChannelId = '';
								mappingChannelName = '';
								mappingConversationId = '';
							}}
							class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
						>
							Cancel
						</button>
					</div>
				</div>
			</div>
		{/if}

		{#if mappings.length === 0}
			<p class="py-4 text-center text-sm text-zinc-400 dark:text-zinc-500">
				No channel mappings. Add one to route messages to a conversation.
			</p>
		{:else}
			<div class="rounded-lg border border-zinc-200 dark:border-zinc-700">
				{#each mappings as mapping, i (mapping.id)}
					<div
						class={[
							'flex items-center gap-3 px-4 py-3',
							i > 0 && 'border-t border-zinc-100 dark:border-zinc-800'
						]}
					>
						<div class="min-w-0 flex-1">
							<div class="flex items-center gap-2 text-sm">
								<span class="font-medium text-zinc-900 dark:text-zinc-100">
									{mapping.external_channel_name || mapping.external_channel_id}
								</span>
								<span class="text-zinc-400 dark:text-zinc-500">→</span>
								<code class="text-xs text-zinc-600 dark:text-zinc-400">
									{mapping.conversation_id}
								</code>
								{#if mapping.is_thread}
									<span
										class="inline-flex rounded-full bg-zinc-100 px-2 py-0.5 text-xs text-zinc-600 dark:bg-zinc-800 dark:text-zinc-400"
									>
										thread
									</span>
								{/if}
							</div>
							<p class="mt-0.5 text-xs text-zinc-400 dark:text-zinc-500">
								ID: {mapping.external_channel_id}
							</p>
						</div>
						<div class="flex items-center gap-1">
							{#if deleteMappingConfirmId === mapping.id}
								<button
									onclick={() => deleteMapping(mapping.id)}
									disabled={deleteMappingInProgress === mapping.id}
									class="rounded px-2 py-1 text-xs text-red-600 hover:bg-red-50 disabled:opacity-50 dark:text-red-400 dark:hover:bg-red-950"
								>
									Confirm
								</button>
								<button
									onclick={() => (deleteMappingConfirmId = null)}
									class="rounded px-2 py-1 text-xs text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800"
								>
									Cancel
								</button>
							{:else}
								<button
									onclick={() => (deleteMappingConfirmId = mapping.id)}
									class="rounded px-2 py-1 text-xs text-red-600 hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-950"
								>
									Remove
								</button>
							{/if}
						</div>
					</div>
				{/each}
			</div>
		{/if}
	</section>

	<!-- User Mappings section -->
	<section>
		<div class="mb-4 flex items-center justify-between">
			<h3 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">User Mappings</h3>
			<button
				onclick={() => (showAddUserForm = !showAddUserForm)}
				class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
			>
				Add User
			</button>
		</div>

		{#if showAddUserForm}
			<div class="mb-4 rounded-lg border border-zinc-200 p-4 dark:border-zinc-700">
				<h4 class="mb-3 text-sm font-medium text-zinc-900 dark:text-zinc-100">New User Mapping</h4>
				<div class="space-y-3">
					<div>
						<label
							for="user-external-id"
							class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
						>
							External User ID
						</label>
						<input
							id="user-external-id"
							type="text"
							bind:value={userExternalId}
							placeholder="123456789"
							class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm font-mono text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
						/>
					</div>
					<div>
						<label
							for="user-external-username"
							class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
						>
							External Username
						</label>
						<input
							id="user-external-username"
							type="text"
							bind:value={userExternalUsername}
							placeholder="@username"
							class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
						/>
					</div>
					<div>
						<label
							for="user-sober-id"
							class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
						>
							Sõber User ID
						</label>
						<input
							id="user-sober-id"
							type="text"
							bind:value={userSoberId}
							placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
							class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm font-mono text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
						/>
					</div>
					<div class="flex gap-2">
						<button
							onclick={addUserMapping}
							disabled={userSubmitting || !userExternalId.trim() || !userSoberId.trim()}
							class="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
						>
							{userSubmitting ? 'Adding...' : 'Add'}
						</button>
						<button
							onclick={() => {
								showAddUserForm = false;
								userExternalId = '';
								userExternalUsername = '';
								userSoberId = '';
							}}
							class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
						>
							Cancel
						</button>
					</div>
				</div>
			</div>
		{/if}

		{#if userMappings.length === 0}
			<p class="py-4 text-center text-sm text-zinc-400 dark:text-zinc-500">
				No user mappings. Add one to link external users to Sõber accounts.
			</p>
		{:else}
			<div class="rounded-lg border border-zinc-200 dark:border-zinc-700">
				{#each userMappings as mapping, i (mapping.id)}
					<div
						class={[
							'flex items-center gap-3 px-4 py-3',
							i > 0 && 'border-t border-zinc-100 dark:border-zinc-800'
						]}
					>
						<div class="min-w-0 flex-1">
							<div class="flex items-center gap-2 text-sm">
								<span class="font-medium text-zinc-900 dark:text-zinc-100">
									{mapping.external_username}
								</span>
								<span class="text-zinc-400 dark:text-zinc-500">→</span>
								<code class="text-xs text-zinc-600 dark:text-zinc-400">
									{mapping.user_id}
								</code>
							</div>
							<p class="mt-0.5 text-xs text-zinc-400 dark:text-zinc-500">
								External ID: {mapping.external_user_id}
							</p>
						</div>
						<div class="flex items-center gap-1">
							{#if deleteUserConfirmId === mapping.id}
								<button
									onclick={() => deleteUserMapping(mapping.id)}
									disabled={deleteUserInProgress === mapping.id}
									class="rounded px-2 py-1 text-xs text-red-600 hover:bg-red-50 disabled:opacity-50 dark:text-red-400 dark:hover:bg-red-950"
								>
									Confirm
								</button>
								<button
									onclick={() => (deleteUserConfirmId = null)}
									class="rounded px-2 py-1 text-xs text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800"
								>
									Cancel
								</button>
							{:else}
								<button
									onclick={() => (deleteUserConfirmId = mapping.id)}
									class="rounded px-2 py-1 text-xs text-red-600 hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-950"
								>
									Remove
								</button>
							{/if}
						</div>
					</div>
				{/each}
			</div>
		{/if}
	</section>
{:else}
	<p class="py-8 text-center text-sm text-zinc-400 dark:text-zinc-500">Platform not found.</p>
{/if}
