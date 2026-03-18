import { api } from '$lib/utils/api';
import type { SkillInfo } from '$lib/types';

export const skillsService = {
	list: () => api<SkillInfo[]>('/skills'),
	reload: () => api<SkillInfo[]>('/skills/reload', { method: 'POST' })
};
