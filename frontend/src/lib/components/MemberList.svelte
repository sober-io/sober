<script lang="ts">
	import type { ConversationMember, ConversationUserRole, ConversationKind } from '$lib/types';
	import AddMemberInput from './AddMemberInput.svelte';

	interface Props {
		members: ConversationMember[];
		currentUserId: string;
		currentUserRole: ConversationUserRole;
		conversationKind: ConversationKind;
		onAddMember: (username: string) => void;
		onUpdateRole: (userId: string, role: string) => void;
		onRemoveMember: (userId: string) => void;
		onLeave: () => void;
	}

	let {
		members,
		currentUserId,
		currentUserRole,
		conversationKind,
		onAddMember,
		onUpdateRole,
		onRemoveMember,
		onLeave
	}: Props = $props();

	let canManage = $derived(currentUserRole === 'owner' || currentUserRole === 'admin');

	function roleBadgeClass(role: ConversationUserRole): string {
		switch (role) {
			case 'owner':
				return 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-400';
			case 'admin':
				return 'bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-400';
			default:
				return 'bg-zinc-100 text-zinc-600 dark:bg-zinc-700 dark:text-zinc-300';
		}
	}

	function canKick(member: ConversationMember): boolean {
		if (member.user_id === currentUserId) return false;
		if (currentUserRole === 'owner') return true;
		if (currentUserRole === 'admin' && member.role === 'member') return true;
		return false;
	}

	function canChangeRole(member: ConversationMember): boolean {
		if (member.user_id === currentUserId) return false;
		if (currentUserRole === 'owner' && member.role !== 'owner') return true;
		return false;
	}

	function handleRoleChange(userId: string, e: Event) {
		const value = (e.target as HTMLSelectElement).value;
		onUpdateRole(userId, value);
	}
</script>

<div class="space-y-3">
	{#if canManage}
		<AddMemberInput onAdd={onAddMember} />
	{/if}

	<ul class="space-y-1">
		{#each members as member (member.user_id)}
			<li class="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm">
				<span class="min-w-0 flex-1 truncate text-zinc-900 dark:text-zinc-100">
					{member.username}
					{#if member.user_id === currentUserId}
						<span class="text-xs text-zinc-400 dark:text-zinc-500">(you)</span>
					{/if}
				</span>

				{#if canChangeRole(member)}
					<select
						value={member.role}
						onchange={(e) => handleRoleChange(member.user_id, e)}
						class="rounded border border-zinc-300 bg-transparent px-1.5 py-0.5 text-xs text-zinc-700 outline-none focus:border-zinc-500 dark:border-zinc-600 dark:text-zinc-300 dark:focus:border-zinc-400"
					>
						<option value="admin">Admin</option>
						<option value="member">Member</option>
					</select>
				{:else}
					<span
						class={['rounded-full px-2 py-0.5 text-xs font-medium', roleBadgeClass(member.role)]}
					>
						{member.role}
					</span>
				{/if}

				{#if canKick(member)}
					<button
						onclick={() => onRemoveMember(member.user_id)}
						class="rounded p-0.5 text-zinc-400 hover:text-red-500 dark:text-zinc-500 dark:hover:text-red-400"
						title="Remove member"
					>
						<svg
							xmlns="http://www.w3.org/2000/svg"
							viewBox="0 0 20 20"
							fill="currentColor"
							class="h-4 w-4"
						>
							<path
								d="M6.28 5.22a.75.75 0 0 0-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 1 0 1.06 1.06L10 11.06l3.72 3.72a.75.75 0 1 0 1.06-1.06L11.06 10l3.72-3.72a.75.75 0 0 0-1.06-1.06L10 8.94 6.28 5.22Z"
							/>
						</svg>
					</button>
				{/if}
			</li>
		{/each}
	</ul>

	{#if currentUserRole === 'owner'}
		<p class="text-xs text-zinc-400 dark:text-zinc-500">Transfer ownership is not yet available.</p>
	{:else if conversationKind === 'group'}
		<button
			onclick={onLeave}
			class="w-full rounded-md border border-red-200 px-3 py-1.5 text-left text-sm text-red-600 hover:bg-red-50 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-950"
		>
			Leave conversation
		</button>
	{/if}
</div>
