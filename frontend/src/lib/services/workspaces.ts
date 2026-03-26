import { api } from '$lib/utils/api';
import type { Workspace } from '$lib/types';

export const workspaceService = {
	list: () => api<Workspace[]>('/workspaces')
};
