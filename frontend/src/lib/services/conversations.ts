import { api } from '$lib/utils/api';
import type {
	AgentMode,
	Collaborator,
	Conversation,
	ConversationSettings,
	Message
} from '$lib/types';

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

	create: () => api<Conversation>('/conversations', { method: 'POST', body: '{}' }),

	updateTitle: (id: string, title: string) =>
		api<{ id: string; title: string }>(`/conversations/${id}`, {
			method: 'PATCH',
			body: JSON.stringify({ title })
		}),

	getSettings: (id: string) => api<ConversationSettings>(`/conversations/${id}/settings`),

	updateSettings: (id: string, settings: Partial<ConversationSettings>) =>
		api<ConversationSettings>(`/conversations/${id}/settings`, {
			method: 'PATCH',
			body: JSON.stringify(settings)
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

	updateAgentMode: (id: string, agentMode: AgentMode) =>
		conversationService.updateSettings(id, { agent_mode: agentMode }),

	listCollaborators: (id: string) => api<Collaborator[]>(`/conversations/${id}/collaborators`),

	addCollaborator: (id: string, username: string) =>
		api<Collaborator>(`/conversations/${id}/collaborators`, {
			method: 'POST',
			body: JSON.stringify({ username })
		}),

	updateCollaboratorRole: (id: string, userId: string, role: string) =>
		api(`/conversations/${id}/collaborators/${userId}`, {
			method: 'PATCH',
			body: JSON.stringify({ role })
		}),

	removeCollaborator: (id: string, userId: string) =>
		api(`/conversations/${id}/collaborators/${userId}`, { method: 'DELETE' }),

	leave: (id: string) => api(`/conversations/${id}/leave`, { method: 'POST' }),

	searchUsers: (query: string) =>
		api<{ id: string; username: string }[]>(`/users/search?q=${encodeURIComponent(query)}`),

	convertToGroup: (id: string, title: string) =>
		api(`/conversations/${id}/convert-to-group`, {
			method: 'POST',
			body: JSON.stringify({ title })
		})
};
