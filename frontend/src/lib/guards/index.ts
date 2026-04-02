import { auth } from '$lib/stores/auth.svelte';
import type { ConversationUserRole, SystemRole } from '$lib/types';
import type { Plugin } from '$lib/types/plugin';

const ROLE_HIERARCHY: Record<ConversationUserRole, number> = {
	owner: 3,
	admin: 2,
	member: 1
};

// --- System-level guards ---

/** Returns true if the current user holds the given system role. */
export function hasRole(role: SystemRole): boolean {
	return auth.user?.roles?.includes(role) ?? false;
}

/** Returns true if the current user is a system admin. */
export function isAdmin(): boolean {
	return hasRole('admin');
}

// --- Conversation-level guards ---

/** Returns true if the user's role meets or exceeds the minimum. */
export function hasConversationRole(
	userRole: ConversationUserRole | undefined,
	minimum: ConversationUserRole
): boolean {
	return (ROLE_HIERARCHY[userRole ?? 'member'] ?? 0) >= ROLE_HIERARCHY[minimum];
}

/** Returns true if the user can manage conversation settings (admin+). */
export function canManageConversation(role?: ConversationUserRole): boolean {
	return hasConversationRole(role, 'admin');
}

/** Returns true if the user can delete a conversation (owner only). */
export function canDeleteConversation(role?: ConversationUserRole): boolean {
	return hasConversationRole(role, 'owner');
}

// --- Plugin-level guards ---

/** Returns true if the user can modify/delete this plugin. */
export function canModifyPlugin(plugin: Plugin): boolean {
	if (plugin.scope === 'system') return isAdmin();
	if (plugin.scope === 'user') return plugin.owner_id === auth.user?.id;
	// workspace: owner or system admin
	return plugin.owner_id === auth.user?.id || isAdmin();
}
