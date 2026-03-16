<script lang="ts">
	import type {
		Conversation,
		Tag,
		Job,
		PermissionMode,
		Workspace,
		AgentMode,
		ConversationMember,
		ConversationUserRole
	} from '$lib/types';
	import { jobService } from '$lib/services/jobs';
	import { workspaceService } from '$lib/services/workspaces';
	import { conversationService } from '$lib/services/conversations';
	import { auth } from '$lib/stores/auth.svelte';
	import { untrack } from 'svelte';
	import { conversations } from '$lib/stores/conversations.svelte';
	import PermissionModeSelector from '$lib/components/PermissionModeSelector.svelte';
	import MemberList from './MemberList.svelte';
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
		onUpdateWorkspace: (workspaceId: string | null) => void;
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
		onUpdateWorkspace,
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

	// Local state
	let editingTitle = $state('');
	let jobs = $state<Job[]>([]);
	let jobsLoading = $state(false);
	let workspaces = $state<Workspace[]>([]);
	let workspacesLoading = $state(false);
	let confirmClear = $state(false);
	let confirmDelete = $state(false);
	let confirmConvertPending = $state<string | null>(null);
	let agentMode = $state<AgentMode>('always');
	let members = $state<ConversationMember[]>([]);
	let membersLoading = $state(false);
	let kind = $state(conversation.kind);

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
		const me = members.find((m) => m.user_id === currentUserId);
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
			loadWorkspaces();
			loadMembers();
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

	async function loadWorkspaces() {
		workspacesLoading = true;
		try {
			workspaces = await workspaceService.list();
		} catch {
			workspaces = [];
		} finally {
			workspacesLoading = false;
		}
	}

	async function loadMembers() {
		membersLoading = true;
		try {
			members = await conversationService.listMembers(conversation.id);
		} catch {
			members = [];
		} finally {
			membersLoading = false;
		}
	}

	async function handleAgentModeChange(mode: AgentMode) {
		agentMode = mode;
		await conversationService.updateAgentMode(conversation.id, mode);
	}

	async function handleAddMember(username: string) {
		if (conversation.kind === 'direct') {
			confirmConvertPending = username;
			return;
		}
		try {
			const member = await conversationService.addMember(conversation.id, username);
			members = [...members, member];
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

			const member = await conversationService.addMember(conversation.id, username);
			members = [...members, member];
		} catch {
			// Could show error toast in the future
		}
	}

	async function handleUpdateRole(userId: string, role: string) {
		try {
			await conversationService.updateMemberRole(conversation.id, userId, role);
			const idx = members.findIndex((m) => m.user_id === userId);
			if (idx !== -1) {
				members[idx] = { ...members[idx], role: role as ConversationUserRole };
			}
		} catch {
			// Could show error toast in the future
		}
	}

	async function handleRemoveMember(userId: string) {
		try {
			await conversationService.removeMember(conversation.id, userId);
			members = members.filter((m) => m.user_id !== userId);
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

	function handleWorkspaceChange(e: Event) {
		const value = (e.target as HTMLSelectElement).value;
		onUpdateWorkspace(value === '' ? null : value);
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

			<!-- Workspace -->
			<SettingsSection title="Workspace" description="Link to a project workspace">
				{#if workspacesLoading}
					<p class="text-xs text-zinc-400 dark:text-zinc-500">Loading...</p>
				{:else}
					<select
						onchange={handleWorkspaceChange}
						value={conversation.workspace_id ?? ''}
						class="w-full rounded-md border border-zinc-300 bg-transparent px-3 py-1.5 text-sm text-zinc-900 outline-none focus:border-zinc-500 dark:border-zinc-600 dark:text-zinc-100 dark:focus:border-zinc-400"
					>
						<option value="">None</option>
						{#each workspaces as ws (ws.id)}
							<option value={ws.id}>{ws.name}</option>
						{/each}
					</select>
				{/if}
			</SettingsSection>

			<!-- Tags -->
			<SettingsSection title="Tags" description="Organize with tags">
				<TagInput {tags} onAdd={onAddTag} onRemove={onRemoveTag} />
			</SettingsSection>

			<!-- Members -->
			<SettingsSection title="Members" description="Manage conversation members">
				{#if membersLoading}
					<p class="text-xs text-zinc-400 dark:text-zinc-500">Loading...</p>
				{:else}
					<MemberList
						{members}
						{currentUserId}
						{currentUserRole}
						conversationKind={conversation.kind}
						onAddMember={handleAddMember}
						onUpdateRole={handleUpdateRole}
						onRemoveMember={handleRemoveMember}
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
	message="Adding members will convert this to a group conversation. Continue?"
	confirmText="Convert & add"
	onConfirm={confirmConvertAndAdd}
	onCancel={() => (confirmConvertPending = null)}
/>
