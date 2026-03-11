import { api } from '$lib/utils/api';
import type { Conversation, ConversationWithMessages } from '$lib/types';

export const conversationService = {
	list: () => api<Conversation[]>('/conversations'),

	get: (id: string) => api<ConversationWithMessages>(`/conversations/${id}`),

	create: () =>
		api<Conversation>('/conversations', {
			method: 'POST',
			body: JSON.stringify({})
		}),

	updateTitle: (id: string, title: string) =>
		api<{ id: string; title: string }>(`/conversations/${id}`, {
			method: 'PATCH',
			body: JSON.stringify({ title })
		}),

	delete: (id: string) =>
		api<{ deleted: boolean }>(`/conversations/${id}`, {
			method: 'DELETE'
		})
};
