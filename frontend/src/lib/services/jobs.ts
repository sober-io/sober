import { api } from '$lib/utils/api';
import type { Job } from '$lib/types';

export const jobService = {
	listByConversation: (conversationId: string) =>
		api<Job[]>(`/conversations/${conversationId}/jobs`),
};
