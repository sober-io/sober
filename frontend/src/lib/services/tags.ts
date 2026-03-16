import { api } from '$lib/utils/api';
import type { Tag } from '$lib/types';

export const tagService = {
	list: () => api<Tag[]>('/tags'),

	addToConversation: (conversationId: string, name: string) =>
		api<Tag>(`/conversations/${conversationId}/tags`, {
			method: 'POST',
			body: JSON.stringify({ name })
		}),

	removeFromConversation: (conversationId: string, tagId: string) =>
		api(`/conversations/${conversationId}/tags/${tagId}`, { method: 'DELETE' }),

	addToMessage: (messageId: string, name: string) =>
		api<Tag>(`/messages/${messageId}/tags`, { method: 'POST', body: JSON.stringify({ name }) }),

	removeFromMessage: (messageId: string, tagId: string) =>
		api(`/messages/${messageId}/tags/${tagId}`, { method: 'DELETE' })
};
