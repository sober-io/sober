import { api } from '$lib/utils/api';
import type { Conversation, Message, PermissionMode } from '$lib/types';

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

	create: () => api<Conversation>('/conversations', { method: 'POST', body: JSON.stringify({}) }),

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

	deleteMessage: (id: string) => api<{ deleted: boolean }>(`/messages/${id}`, { method: 'DELETE' })
};
