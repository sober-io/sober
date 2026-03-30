import { api } from '$lib/utils/api';
import type { EvolutionEvent, EvolutionConfig } from '$lib/types';

export const evolutionService = {
	list: (type?: string, status?: string) => {
		const params = new URLSearchParams();
		if (type) params.set('type', type);
		if (status) params.set('status', status);
		const qs = params.toString();
		return api<EvolutionEvent[]>(`/evolution${qs ? `?${qs}` : ''}`);
	},

	get: (id: string) => api<EvolutionEvent>(`/evolution/${id}`),

	update: (id: string, status: string) =>
		api<EvolutionEvent>(`/evolution/${id}`, {
			method: 'PATCH',
			body: JSON.stringify({ status })
		}),

	getConfig: () => api<EvolutionConfig>('/evolution/config'),

	updateConfig: (config: Partial<EvolutionConfig>) =>
		api<EvolutionConfig>('/evolution/config', {
			method: 'PATCH',
			body: JSON.stringify(config)
		}),

	timeline: (limit = 50, type?: string, status?: string) => {
		const params = new URLSearchParams({ limit: String(limit) });
		if (type) params.set('type', type);
		if (status) params.set('status', status);
		return api<EvolutionEvent[]>(`/evolution/timeline?${params}`);
	}
};
