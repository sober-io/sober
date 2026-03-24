import { api } from '$lib/utils/api';

export const systemService = {
	status: () => api<{ initialized: boolean }>('/system/status')
};
