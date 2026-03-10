import { api } from '$lib/utils/api';
import type { Conversation, ConversationWithMessages } from '$lib/types';

export const conversationService = {
	list: () => api<Conversation[]>('/conversations'),

	get: (id: string) => api<ConversationWithMessages>(`/conversations/${id}`),

	create: () =>
		api<Conversation>('/conversations', {
			method: 'POST',
			body: JSON.stringify({})
		})
};
