import { api } from '$lib/utils/api';
import type { ToolInfo } from '$lib/types';

export const toolService = {
	list: () => api<ToolInfo[]>('/tools')
};
