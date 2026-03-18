import { api } from '$lib/utils/api';
import type { SkillInfo } from '$lib/types';

export const skillsService = {
	list: (conversationId?: string) =>
		api<SkillInfo[]>(conversationId ? `/skills?conversation_id=${conversationId}` : '/skills'),
	reload: (conversationId?: string) =>
		api<SkillInfo[]>(
			conversationId ? `/skills/reload?conversation_id=${conversationId}` : '/skills/reload',
			{ method: 'POST' }
		)
};
