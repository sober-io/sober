import { api } from '$lib/utils/api';
import type { AgentMode, Conversation, ConversationMember, Message, PermissionMode } from '$lib/types';

export const conversationService = {
	list: (params?: { archived?: boolean; kind?: string; tag?: string; search?: string }) => {
		const searchParams = new URLSearchParams();
		if (params?.archived !== undefined) searchParams.set('archived', String(params.archived));
		if (params?.kind) searchParams.set('kind', params.kind);
		if (params?.tag) searchParams.set('tag', params.tag);
		if (params?.search) searchParams.set('search', params.search);
		const query = searchParams.toString();
		return api<Conversation[]>(`/conversations${query ? `?${query}` : ''}`);
	},

	get: (id: string) => api<Conversation>(`/conversations/${id}`),

	create: (params?: { kind?: string; title?: string; members?: { username: string }[] }) =>
		api<Conversation>('/conversations', {
			method: 'POST',
			body: JSON.stringify(params ?? {})
		}),

	updateTitle: (id: string, title: string) =>
		api<{ id: string; title: string }>(`/conversations/${id}`, {
			method: 'PATCH',
			body: JSON.stringify({ title })
		}),

	updatePermissionMode: (id: string, mode: PermissionMode) =>
		api<{ id: string; permission_mode: PermissionMode }>(`/conversations/${id}`, {
			method: 'PATCH',
			body: JSON.stringify({ permission_mode: mode })
		}),

	archive: (id: string, archived: boolean) =>
		api(`/conversations/${id}`, { method: 'PATCH', body: JSON.stringify({ archived }) }),

	delete: (id: string) => api<{ deleted: boolean }>(`/conversations/${id}`, { method: 'DELETE' }),

	getInbox: () => api<Conversation>('/conversations/inbox'),

	markRead: (id: string) => api('/conversations/' + id + '/read', { method: 'POST' }),

	clearMessages: (id: string) => api(`/conversations/${id}/messages`, { method: 'DELETE' }),

	listMessages: (id: string, before?: string, limit = 50) => {
		const params = new URLSearchParams({ limit: String(limit) });
		if (before) params.set('before', before);
		return api<Message[]>(`/conversations/${id}/messages?${params}`);
	},

	deleteMessage: (id: string) => api<{ deleted: boolean }>(`/messages/${id}`, { method: 'DELETE' }),

	updateWorkspace: (id: string, workspaceId: string | null) =>
		api(`/conversations/${id}`, {
			method: 'PATCH',
			body: JSON.stringify({ workspace_id: workspaceId })
		}),

	updateAgentMode: (id: string, agentMode: AgentMode) =>
		api(`/conversations/${id}`, {
			method: 'PATCH',
			body: JSON.stringify({ agent_mode: agentMode })
		}),

	listMembers: (id: string) => api<ConversationMember[]>(`/conversations/${id}/members`),

	addMember: (id: string, username: string) =>
		api<ConversationMember>(`/conversations/${id}/members`, {
			method: 'POST',
			body: JSON.stringify({ username })
		}),

	updateMemberRole: (id: string, userId: string, role: string) =>
		api(`/conversations/${id}/members/${userId}`, {
			method: 'PATCH',
			body: JSON.stringify({ role })
		}),

	removeMember: (id: string, userId: string) =>
		api(`/conversations/${id}/members/${userId}`, { method: 'DELETE' }),

	leave: (id: string) => api(`/conversations/${id}/leave`, { method: 'POST' })
};
