<script lang="ts">
	import type { Conversation, Tag, Job, PermissionMode, Workspace } from '$lib/types';
	import { jobService } from '$lib/services/jobs';
	import { workspaceService } from '$lib/services/workspaces';
	import { PERMISSION_MODES } from '$lib/constants/permission-modes';
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

	// Local state
	let editingTitle = $state('');
	let jobs = $state<Job[]>([]);
	let jobsLoading = $state(false);
	let workspaces = $state<Workspace[]>([]);
	let workspacesLoading = $state(false);
	let confirmClear = $state(false);
	let confirmDelete = $state(false);

	// Derived
	let createdDate = $derived(
		new Date(conversation.created_at).toLocaleDateString(undefined, {
			year: 'numeric',
			month: 'long',
			day: 'numeric'
		})
	);
	let kindLabel = $derived(
		conversation.kind === 'inbox' ? 'Inbox' : conversation.kind === 'group' ? 'Group' : 'Direct'
	);

	// Load data when panel opens
	$effect(() => {
		if (open) {
			editingTitle = conversation.title ?? '';
			loadJobs();
			loadWorkspaces();
		}
	});

	// Close on Escape
	$effect(() => {
		if (!open) return;
		const handler = (e: KeyboardEvent) => {
			if (e.key === 'Escape' && !confirmClear && !confirmDelete) {
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
				<div class="flex gap-1">
					{#each PERMISSION_MODES as mode (mode.value)}
						<button
							onclick={() => onUpdatePermissionMode(mode.value)}
							class={[
								'flex-1 rounded-md px-2 py-1.5 text-center text-xs font-medium transition-colors',
								permissionMode === mode.value
									? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900'
									: 'bg-zinc-100 text-zinc-600 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-400 dark:hover:bg-zinc-700'
							]}
							title={mode.description}
						>
							{mode.label}
						</button>
					{/each}
				</div>
			</SettingsSection>

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
