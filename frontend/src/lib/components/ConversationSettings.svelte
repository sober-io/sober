<script lang="ts">
	import type {
		Conversation,
		ConversationSettings as ConversationSettingsType,
		Tag,
		Job,
		PermissionMode,
		AgentMode,
		SandboxNetMode,
		Collaborator,
		ConversationUserRole,
		ToolInfo
	} from '$lib/types';
	import type { Plugin } from '$lib/types/plugin';
	import { jobService } from '$lib/services/jobs';
	import { conversationService } from '$lib/services/conversations';
	import { toolService } from '$lib/services/tools';
	import { pluginService } from '$lib/services/plugins';
	import { auth } from '$lib/stores/auth.svelte';
	import { untrack } from 'svelte';
	import { conversations } from '$lib/stores/conversations.svelte';
	import PermissionModeSelector from '$lib/components/PermissionModeSelector.svelte';
	import CollaboratorList from './CollaboratorList.svelte';
	import TagInput from './TagInput.svelte';
	import ConfirmDialog from './ConfirmDialog.svelte';
	import SettingsSection from './SettingsSection.svelte';
	import JobList from './JobList.svelte';

	interface Props {
		open: boolean;
		conversation: Conversation;
		tags: Tag[];
		permissionMode: PermissionMode;
		onClose: () => void;
		onUpdateTitle: (title: string) => void;
		onUpdatePermissionMode: (mode: PermissionMode) => void;
		onAddTag: (name: string) => void;
		onRemoveTag: (tagId: string) => void;
		onArchive: () => void;
		onClearHistory: () => void;
		onDelete: () => void;
	}

	let {
		open,
		conversation,
		tags,
		permissionMode,
		onClose,
		onUpdateTitle,
		onUpdatePermissionMode,
		onAddTag,
		onRemoveTag,
		onArchive,
		onClearHistory,
		onDelete
	}: Props = $props();

	// Agent mode options
	const AGENT_MODES: ReadonlyArray<{
		value: AgentMode;
		label: string;
		description: string;
		color: string;
	}> = [
		{
			value: 'always',
			label: 'Always',
			description: 'Agent responds to every message',
			color: 'emerald'
		},
		{
			value: 'mention',
			label: 'Mention',
			description: 'Agent responds only when mentioned',
			color: 'amber'
		},
		{ value: 'silent', label: 'Silent', description: 'Agent does not respond', color: 'zinc' }
	];

	const SANDBOX_PROFILES = [
		{ value: 'standard', label: 'Standard', description: 'Balanced defaults' },
		{ value: 'locked_down', label: 'Locked Down', description: 'Most restrictive' },
		{ value: 'unrestricted', label: 'Unrestricted', description: 'Least restrictive' }
	] as const;

	const NET_MODES: ReadonlyArray<{ value: SandboxNetMode; label: string }> = [
		{ value: 'none', label: 'None' },
		{ value: 'allowed_domains', label: 'Allowed Domains' },
		{ value: 'full', label: 'Full' }
	];

	// Local state
	let editingTitle = $state('');
	let jobs = $state<Job[]>([]);
	let jobsLoading = $state(false);
	let confirmClear = $state(false);
	let confirmDelete = $state(false);
	let confirmConvertPending = $state<string | null>(null);
	let agentMode = $state<AgentMode>('always');
	let collaborators = $state<Collaborator[]>([]);
	let collaboratorsLoading = $state(false);
	let kind = $state(conversation.kind);

	// Sandbox / workspace settings (loaded from API)
	let settingsLoading = $state(false);
	let sandboxProfile = $state('standard');
	let sandboxNetMode = $state<SandboxNetMode | undefined>(undefined);
	let sandboxAllowedDomains = $state<string[]>([]);
	let sandboxTimeout = $state<number | undefined>(undefined);
	let sandboxAllowSpawn = $state<boolean | undefined>(undefined);
	let autoSnapshot = $state(true);
	let newDomain = $state('');

	// Capabilities (tool/plugin filtering)
	let availableTools = $state<ToolInfo[]>([]);
	let availablePlugins = $state<Plugin[]>([]);
	let disabledTools = $state<string[]>([]);
	let disabledPlugins = $state<string[]>([]);
	let customToolName = $state('');

	// Derived
	let createdDate = $derived(
		new Date(conversation.created_at).toLocaleDateString(undefined, {
			year: 'numeric',
			month: 'long',
			day: 'numeric'
		})
	);
	let kindLabel = $derived(kind === 'inbox' ? 'Inbox' : kind === 'group' ? 'Group' : 'Direct');
	let isGroup = $derived(kind === 'group');
	let currentUserId = $derived(auth.user?.id ?? '');
	let currentUserRole = $derived.by((): ConversationUserRole => {
		const me = collaborators.find((c) => c.user_id === currentUserId);
		return me?.role ?? 'member';
	});
	let canEditAgentMode = $derived(currentUserRole === 'owner' || currentUserRole === 'admin');

	// Load data when panel opens — only track `open`, not conversation fields
	$effect(() => {
		if (!open) return;
		untrack(() => {
			editingTitle = conversation.title ?? '';
			agentMode = conversation.agent_mode ?? 'always';
			kind = conversation.kind;
			loadJobs();
			loadCollaborators();
			loadSettings();
		});
	});

	// Close on Escape
	$effect(() => {
		if (!open) return;
		const handler = (e: KeyboardEvent) => {
			if (e.key === 'Escape' && !confirmClear && !confirmDelete && !confirmConvertPending) {
				onClose();
			}
		};
		document.addEventListener('keydown', handler);
		return () => document.removeEventListener('keydown', handler);
	});

	async function loadJobs() {
		jobsLoading = true;
		try {
			jobs = await jobService.listByConversation(conversation.id);
		} catch {
			jobs = [];
		} finally {
			jobsLoading = false;
		}
	}

	async function loadCollaborators() {
		collaboratorsLoading = true;
		try {
			collaborators = await conversationService.listCollaborators(conversation.id);
		} catch {
			collaborators = [];
		} finally {
			collaboratorsLoading = false;
		}
	}

	async function loadSettings() {
		settingsLoading = true;
		try {
			const s = await conversationService.getSettings(conversation.id);
			sandboxProfile = s.sandbox_profile;
			sandboxNetMode = s.sandbox_net_mode;
			sandboxAllowedDomains = s.sandbox_allowed_domains ?? [];
			sandboxTimeout = s.sandbox_max_execution_seconds;
			sandboxAllowSpawn = s.sandbox_allow_spawn;
			autoSnapshot = s.auto_snapshot;
			disabledTools = s.disabled_tools ?? [];
			disabledPlugins = s.disabled_plugins ?? [];
		} catch {
			// Settings may not exist for legacy conversations
		} finally {
			settingsLoading = false;
		}

		// Load available tools and plugins for the capabilities section
		try {
			const [tools, plugins] = await Promise.all([toolService.list(), pluginService.list()]);
			availableTools = tools;
			availablePlugins = plugins;
		} catch {
			// Non-critical — capabilities section just won't show items
		}
	}

	async function saveSetting(patch: Partial<ConversationSettingsType>) {
		try {
			await conversationService.updateSettings(conversation.id, patch);
		} catch {
			// Could show error toast in the future
		}
	}

	function toggleTool(name: string) {
		if (disabledTools.includes(name)) {
			disabledTools = disabledTools.filter((t) => t !== name);
		} else {
			disabledTools = [...disabledTools, name];
		}
		saveSetting({ disabled_tools: disabledTools });
	}

	function togglePlugin(id: string) {
		if (disabledPlugins.includes(id)) {
			disabledPlugins = disabledPlugins.filter((p) => p !== id);
		} else {
			disabledPlugins = [...disabledPlugins, id];
		}
		saveSetting({ disabled_plugins: disabledPlugins });
	}

	function addCustomTool() {
		const name = customToolName.trim();
		if (!name || disabledTools.includes(name)) return;
		disabledTools = [...disabledTools, name];
		customToolName = '';
		saveSetting({ disabled_tools: disabledTools });
	}

	async function handleProfileChange(profile: string) {
		sandboxProfile = profile;
		await saveSetting({ sandbox_profile: profile });
	}

	async function handleNetModeChange(mode: SandboxNetMode) {
		sandboxNetMode = mode;
		await saveSetting({ sandbox_net_mode: mode });
	}

	function addDomain() {
		const d = newDomain.trim().toLowerCase();
		if (d && !sandboxAllowedDomains.includes(d)) {
			sandboxAllowedDomains = [...sandboxAllowedDomains, d];
			newDomain = '';
			saveSetting({ sandbox_allowed_domains: sandboxAllowedDomains });
		}
	}

	function removeDomain(domain: string) {
		sandboxAllowedDomains = sandboxAllowedDomains.filter((d) => d !== domain);
		saveSetting({ sandbox_allowed_domains: sandboxAllowedDomains });
	}

	async function handleTimeoutChange(e: Event) {
		const val = parseInt((e.target as HTMLInputElement).value, 10);
		if (!isNaN(val) && val > 0) {
			sandboxTimeout = val;
			await saveSetting({ sandbox_max_execution_seconds: val });
		}
	}

	async function handleSpawnToggle() {
		sandboxAllowSpawn = !sandboxAllowSpawn;
		await saveSetting({ sandbox_allow_spawn: sandboxAllowSpawn });
	}

	async function handleAutoSnapshotToggle() {
		autoSnapshot = !autoSnapshot;
		await saveSetting({ auto_snapshot: autoSnapshot });
	}

	async function handleAgentModeChange(mode: AgentMode) {
		agentMode = mode;
		await conversationService.updateAgentMode(conversation.id, mode);
	}

	async function handleAddCollaborator(username: string) {
		if (kind === 'direct') {
			confirmConvertPending = username;
			return;
		}
		try {
			const collaborator = await conversationService.addCollaborator(conversation.id, username);
			if (!collaborators.some((c) => c.user_id === collaborator.user_id)) {
				collaborators = [...collaborators, collaborator];
			}
		} catch {
			// Could show error toast in the future
		}
	}

	async function confirmConvertAndAdd() {
		if (!confirmConvertPending) return;
		const username = confirmConvertPending;
		confirmConvertPending = null;

		try {
			const title = conversation.title || 'Group conversation';
			await conversationService.convertToGroup(conversation.id, title);
			kind = 'group';
			conversations.update(conversation.id, { kind: 'group', title });

			const collaborator = await conversationService.addCollaborator(conversation.id, username);
			if (!collaborators.some((c) => c.user_id === collaborator.user_id)) {
				collaborators = [...collaborators, collaborator];
			}
		} catch {
			// Could show error toast in the future
		}
	}

	async function handleUpdateRole(userId: string, role: string) {
		try {
			await conversationService.updateCollaboratorRole(conversation.id, userId, role);
			const idx = collaborators.findIndex((c) => c.user_id === userId);
			if (idx !== -1) {
				collaborators[idx] = { ...collaborators[idx], role: role as ConversationUserRole };
			}
		} catch {
			// Could show error toast in the future
		}
	}

	async function handleRemoveCollaborator(userId: string) {
		try {
			await conversationService.removeCollaborator(conversation.id, userId);
			collaborators = collaborators.filter((c) => c.user_id !== userId);
			// Auto-convert back to direct if only owner remains
			if (collaborators.length <= 1) {
				kind = 'direct';
				conversations.update(conversation.id, { kind: 'direct' });
			}
		} catch {
			// Could show error toast in the future
		}
	}

	async function handleLeave() {
		try {
			await conversationService.leave(conversation.id);
			onClose();
		} catch {
			// Could show error toast in the future
		}
	}

	function agentModeButtonClass(m: (typeof AGENT_MODES)[number]): string {
		if (agentMode !== m.value) {
			return 'text-zinc-500 hover:text-zinc-700 dark:text-zinc-400 dark:hover:text-zinc-200';
		}
		switch (m.color) {
			case 'emerald':
				return 'bg-emerald-600/30 text-emerald-400';
			case 'amber':
				return 'bg-amber-600/30 text-amber-400';
			default:
				return 'bg-zinc-600/30 text-zinc-300';
		}
	}

	function handleTitleSave() {
		const trimmed = editingTitle.trim();
		if (trimmed && trimmed !== (conversation.title ?? '')) {
			onUpdateTitle(trimmed);
		}
	}

	function handleTitleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			e.preventDefault();
			(e.target as HTMLInputElement).blur();
		}
	}

	function handleClearConfirm() {
		confirmClear = false;
		onClearHistory();
	}

	function handleDeleteConfirm() {
		confirmDelete = false;
		onDelete();
	}
</script>

{#if open}
	<!-- Backdrop -->
	<button
		class="fixed inset-0 z-40 bg-black/30 dark:bg-black/50"
		onclick={onClose}
		aria-label="Close settings"
		tabindex="-1"
	></button>

	<!-- Panel -->
	<div
		class="fixed right-0 top-0 z-50 flex h-full w-full flex-col border-l border-zinc-200 bg-white shadow-xl md:w-[400px] dark:border-zinc-700 dark:bg-zinc-900"
		role="dialog"
		aria-label="Conversation settings"
	>
		<!-- Header -->
		<div
			class="flex items-center justify-between border-b border-zinc-200 px-4 py-3 dark:border-zinc-700"
		>
			<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">Settings</h2>
			<button
				onclick={onClose}
				class="rounded-md p-1 text-zinc-400 hover:text-zinc-600 dark:text-zinc-500 dark:hover:text-zinc-300"
				aria-label="Close"
			>
				<svg
					xmlns="http://www.w3.org/2000/svg"
					viewBox="0 0 20 20"
					fill="currentColor"
					class="h-5 w-5"
				>
					<path
						d="M6.28 5.22a.75.75 0 0 0-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 1 0 1.06 1.06L10 11.06l3.72 3.72a.75.75 0 1 0 1.06-1.06L11.06 10l3.72-3.72a.75.75 0 0 0-1.06-1.06L10 8.94 6.28 5.22Z"
					/>
				</svg>
			</button>
		</div>

		<!-- Content -->
		<div class="flex-1 divide-y divide-zinc-200 overflow-y-auto px-4 dark:divide-zinc-700">
			<!-- Info -->
			<SettingsSection title="Info" description="Conversation details">
				<div class="flex items-center gap-3 text-sm text-zinc-600 dark:text-zinc-400">
					<span
						class="rounded-full bg-zinc-100 px-2 py-0.5 text-xs font-medium text-zinc-600 dark:bg-zinc-700 dark:text-zinc-300"
					>
						{kindLabel}
					</span>
					<span class="text-xs">Created {createdDate}</span>
				</div>
			</SettingsSection>

			<!-- Title -->
			<SettingsSection title="Title">
				<input
					type="text"
					bind:value={editingTitle}
					onblur={handleTitleSave}
					onkeydown={handleTitleKeydown}
					placeholder="Conversation title..."
					class="w-full rounded-md border border-zinc-300 bg-transparent px-3 py-1.5 text-sm text-zinc-900 outline-none placeholder:text-zinc-400 focus:border-zinc-500 dark:border-zinc-600 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400"
				/>
			</SettingsSection>

			<!-- Permission mode -->
			<SettingsSection title="Permission mode" description="Controls agent autonomy level">
				<PermissionModeSelector mode={permissionMode} onModeChange={onUpdatePermissionMode} />
			</SettingsSection>

			<!-- Agent mode (group only) -->
			{#if isGroup}
				<SettingsSection title="Agent mode" description="When the agent responds in this group">
					{#if canEditAgentMode}
						<div class="flex w-full gap-1 rounded-lg bg-zinc-100 p-1 dark:bg-zinc-800">
							{#each AGENT_MODES as m (m.value)}
								<button
									onclick={() => handleAgentModeChange(m.value)}
									class={[
										'flex-1 rounded-md px-3 py-2 text-sm font-medium transition-colors',
										agentModeButtonClass(m)
									]}
									title={m.description}
								>
									{m.label}
								</button>
							{/each}
						</div>
					{:else}
						<div class="flex items-center gap-2">
							<span
								class="rounded-full bg-zinc-100 px-2 py-0.5 text-xs font-medium text-zinc-600 dark:bg-zinc-700 dark:text-zinc-300"
							>
								{AGENT_MODES.find((m) => m.value === agentMode)?.label ?? agentMode}
							</span>
							<span class="text-xs text-zinc-500 dark:text-zinc-400">
								{AGENT_MODES.find((m) => m.value === agentMode)?.description ?? ''}
							</span>
						</div>
					{/if}
				</SettingsSection>
			{/if}

			<!-- Sandbox -->
			<SettingsSection title="Sandbox" description="Execution security policy">
				{#if settingsLoading}
					<p class="text-xs text-zinc-400 dark:text-zinc-500">Loading...</p>
				{:else}
					<div class="space-y-3">
						<!-- Profile -->
						<div>
							<label class="mb-1 block text-xs font-medium text-zinc-500 dark:text-zinc-400"
								>Profile</label
							>
							<div class="flex w-full gap-1 rounded-lg bg-zinc-100 p-1 dark:bg-zinc-800">
								{#each SANDBOX_PROFILES as p (p.value)}
									<button
										onclick={() => handleProfileChange(p.value)}
										class={[
											'flex-1 rounded-md px-2 py-1.5 text-xs font-medium transition-colors',
											sandboxProfile === p.value
												? 'bg-zinc-700 text-white dark:bg-zinc-600'
												: 'text-zinc-500 hover:text-zinc-700 dark:text-zinc-400 dark:hover:text-zinc-200'
										]}
										title={p.description}
									>
										{p.label}
									</button>
								{/each}
							</div>
						</div>

						<!-- Network mode -->
						<div>
							<label class="mb-1 block text-xs font-medium text-zinc-500 dark:text-zinc-400"
								>Network</label
							>
							<div class="flex w-full gap-1 rounded-lg bg-zinc-100 p-1 dark:bg-zinc-800">
								{#each NET_MODES as m (m.value)}
									<button
										onclick={() => handleNetModeChange(m.value)}
										class={[
											'flex-1 rounded-md px-2 py-1.5 text-xs font-medium transition-colors',
											sandboxNetMode === m.value
												? 'bg-zinc-700 text-white dark:bg-zinc-600'
												: 'text-zinc-500 hover:text-zinc-700 dark:text-zinc-400 dark:hover:text-zinc-200'
										]}
									>
										{m.label}
									</button>
								{/each}
							</div>
						</div>

						<!-- Allowed domains (shown only when net mode is allowed_domains) -->
						{#if sandboxNetMode === 'allowed_domains'}
							<div>
								<label class="mb-1 block text-xs font-medium text-zinc-500 dark:text-zinc-400"
									>Allowed domains</label
								>
								<div class="space-y-1">
									{#each sandboxAllowedDomains as domain (domain)}
										<div
											class="flex items-center justify-between rounded bg-zinc-100 px-2 py-1 dark:bg-zinc-800"
										>
											<span class="font-mono text-xs text-zinc-700 dark:text-zinc-300"
												>{domain}</span
											>
											<button
												onclick={() => removeDomain(domain)}
												class="text-zinc-400 hover:text-red-500 dark:text-zinc-500 dark:hover:text-red-400"
												aria-label="Remove {domain}"
											>
												<svg viewBox="0 0 20 20" fill="currentColor" class="h-3.5 w-3.5">
													<path
														d="M6.28 5.22a.75.75 0 0 0-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 1 0 1.06 1.06L10 11.06l3.72 3.72a.75.75 0 1 0 1.06-1.06L11.06 10l3.72-3.72a.75.75 0 0 0-1.06-1.06L10 8.94 6.28 5.22Z"
													/>
												</svg>
											</button>
										</div>
									{/each}
									<form
										class="flex gap-1"
										onsubmit={(e) => {
											e.preventDefault();
											addDomain();
										}}
									>
										<input
											type="text"
											bind:value={newDomain}
											placeholder="github.com"
											class="flex-1 rounded-md border border-zinc-300 bg-transparent px-2 py-1 font-mono text-xs text-zinc-900 outline-none placeholder:text-zinc-400 focus:border-zinc-500 dark:border-zinc-600 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400"
										/>
										<button
											type="submit"
											class="rounded-md bg-zinc-200 px-2 py-1 text-xs font-medium text-zinc-700 hover:bg-zinc-300 dark:bg-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-600"
										>
											Add
										</button>
									</form>
								</div>
							</div>
						{/if}

						<!-- Timeout + Spawn row -->
						<div class="flex items-center gap-3">
							<div class="flex-1">
								<label class="mb-1 block text-xs font-medium text-zinc-500 dark:text-zinc-400"
									>Timeout (s)</label
								>
								<input
									type="number"
									min="1"
									max="3600"
									value={sandboxTimeout ?? ''}
									onchange={handleTimeoutChange}
									placeholder="default"
									class="w-full rounded-md border border-zinc-300 bg-transparent px-2 py-1 text-xs text-zinc-900 outline-none placeholder:text-zinc-400 focus:border-zinc-500 dark:border-zinc-600 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400"
								/>
							</div>
							<label class="flex items-center gap-2 pt-4">
								<input
									type="checkbox"
									checked={sandboxAllowSpawn ?? false}
									onchange={handleSpawnToggle}
									class="h-3.5 w-3.5 rounded border-zinc-300 text-zinc-600 dark:border-zinc-600"
								/>
								<span class="text-xs text-zinc-600 dark:text-zinc-400">Allow spawn</span>
							</label>
						</div>

						<!-- Auto-snapshot -->
						<label class="flex items-center gap-2">
							<input
								type="checkbox"
								checked={autoSnapshot}
								onchange={handleAutoSnapshotToggle}
								class="h-3.5 w-3.5 rounded border-zinc-300 text-zinc-600 dark:border-zinc-600"
							/>
							<span class="text-xs text-zinc-600 dark:text-zinc-400"
								>Auto-snapshot before dangerous commands</span
							>
						</label>
					</div>
				{/if}
			</SettingsSection>

			<!-- Capabilities -->
			<SettingsSection
				title="Capabilities"
				description="Control which tools and plugins the agent can use"
			>
				{#if settingsLoading}
					<p class="text-xs text-zinc-400 dark:text-zinc-500">Loading...</p>
				{:else}
					<div class="space-y-4">
						<!-- Plugins -->
						{#if availablePlugins.length > 0}
							<div>
								<p class="mb-2 text-xs font-medium text-zinc-500 dark:text-zinc-400">Plugins</p>
								<div class="space-y-1">
									{#each availablePlugins as plugin}
										<label class="flex items-center gap-2">
											<input
												type="checkbox"
												checked={!disabledPlugins.includes(plugin.id)}
												onchange={() => togglePlugin(plugin.id)}
												class="h-3.5 w-3.5 rounded border-zinc-300 text-zinc-600 dark:border-zinc-600"
											/>
											<span class="text-xs text-zinc-700 dark:text-zinc-300">
												{plugin.name}
											</span>
											<span class="text-xs text-zinc-400 dark:text-zinc-500">
												{plugin.kind}
											</span>
										</label>
									{/each}
								</div>
							</div>
						{/if}

						<!-- Tools -->
						{#if availableTools.length > 0}
							<div>
								<p class="mb-2 text-xs font-medium text-zinc-500 dark:text-zinc-400">Tools</p>
								<div class="space-y-1">
									{#each availableTools as tool}
										<label class="flex items-center gap-2">
											<input
												type="checkbox"
												checked={!disabledTools.includes(tool.name)}
												onchange={() => toggleTool(tool.name)}
												class="h-3.5 w-3.5 rounded border-zinc-300 text-zinc-600 dark:border-zinc-600"
											/>
											<span class="text-xs text-zinc-700 dark:text-zinc-300">
												{tool.name}
											</span>
											{#if tool.plugin_name}
												<span class="text-xs text-zinc-400 dark:text-zinc-500">
													({tool.plugin_name})
												</span>
											{/if}
										</label>
									{/each}
								</div>
							</div>
						{/if}

						<!-- Custom tool name (power user) -->
						<div>
							<p class="mb-1 text-xs font-medium text-zinc-500 dark:text-zinc-400">
								Disable by name
							</p>
							<div class="flex gap-1">
								<input
									type="text"
									bind:value={customToolName}
									placeholder="tool_name"
									onkeydown={(e: KeyboardEvent) => {
										if (e.key === 'Enter') addCustomTool();
									}}
									class="flex-1 rounded-md border border-zinc-300 bg-transparent px-2 py-1 text-xs text-zinc-900 outline-none placeholder:text-zinc-400 focus:border-zinc-500 dark:border-zinc-600 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400"
								/>
								<button
									onclick={addCustomTool}
									class="rounded-md bg-zinc-200 px-2 py-1 text-xs text-zinc-700 hover:bg-zinc-300 dark:bg-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-600"
								>
									Add
								</button>
							</div>
						</div>
					</div>
				{/if}
			</SettingsSection>

			<!-- Workspace -->
			<SettingsSection title="Workspace" description="Linked project workspace">
				{#if conversation.workspace_name}
					<p class="text-sm text-zinc-900 dark:text-zinc-100">{conversation.workspace_name}</p>
					{#if conversation.workspace_path}
						<p class="mt-0.5 font-mono text-xs text-zinc-400 dark:text-zinc-500">
							{conversation.workspace_path}
						</p>
					{/if}
				{:else}
					<p class="text-sm text-zinc-400 dark:text-zinc-500">None</p>
				{/if}
			</SettingsSection>

			<!-- Tags -->
			<SettingsSection title="Tags" description="Organize with tags">
				<TagInput {tags} onAdd={onAddTag} onRemove={onRemoveTag} />
			</SettingsSection>

			<!-- Collaborators -->
			<SettingsSection title="Collaborators" description="Manage conversation collaborators">
				{#if collaboratorsLoading}
					<p class="text-xs text-zinc-400 dark:text-zinc-500">Loading...</p>
				{:else}
					<CollaboratorList
						{collaborators}
						{currentUserId}
						{currentUserRole}
						conversationKind={conversation.kind}
						onAddCollaborator={handleAddCollaborator}
						onUpdateRole={handleUpdateRole}
						onRemoveCollaborator={handleRemoveCollaborator}
						onLeave={handleLeave}
					/>
				{/if}
			</SettingsSection>

			<!-- Scheduled jobs -->
			<SettingsSection title="Scheduled jobs" description="Automated tasks for this conversation">
				<JobList {jobs} loading={jobsLoading} />
			</SettingsSection>

			<!-- Danger zone -->
			<SettingsSection title="Danger zone" danger>
				<div class="space-y-2">
					<button
						onclick={onArchive}
						class="w-full rounded-md border border-zinc-300 px-3 py-1.5 text-left text-sm text-zinc-700 hover:bg-zinc-50 dark:border-zinc-600 dark:text-zinc-300 dark:hover:bg-zinc-800"
					>
						{conversation.is_archived ? 'Unarchive conversation' : 'Archive conversation'}
					</button>
					<button
						onclick={() => (confirmClear = true)}
						class="w-full rounded-md border border-red-200 px-3 py-1.5 text-left text-sm text-red-600 hover:bg-red-50 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-950"
					>
						Clear message history
					</button>
					{#if conversation.kind !== 'inbox'}
						<button
							onclick={() => (confirmDelete = true)}
							class="w-full rounded-md border border-red-200 bg-red-50 px-3 py-1.5 text-left text-sm font-medium text-red-600 hover:bg-red-100 dark:border-red-800 dark:bg-red-950 dark:text-red-400 dark:hover:bg-red-900"
						>
							Delete conversation
						</button>
					{/if}
				</div>
			</SettingsSection>
		</div>
	</div>
{/if}

<ConfirmDialog
	open={confirmClear}
	title="Clear message history"
	message="All messages in this conversation will be permanently deleted. This action cannot be undone."
	confirmText="Clear history"
	destructive
	onConfirm={handleClearConfirm}
	onCancel={() => (confirmClear = false)}
/>

<ConfirmDialog
	open={confirmDelete}
	title="Delete conversation"
	message="This conversation and all its messages will be permanently deleted. This action cannot be undone."
	confirmText="Delete"
	destructive
	onConfirm={handleDeleteConfirm}
	onCancel={() => (confirmDelete = false)}
/>

<ConfirmDialog
	open={confirmConvertPending !== null}
	title="Convert to group"
	message="Adding collaborators will convert this to a group conversation. Continue?"
	confirmText="Convert & add"
	onConfirm={confirmConvertAndAdd}
	onCancel={() => (confirmConvertPending = null)}
/>
