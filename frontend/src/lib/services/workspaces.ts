import { api } from '$lib/utils/api';
import type { PermissionMode, Workspace, WorkspaceSettings } from '$lib/types';

export const workspaceService = {
	list: () => api<Workspace[]>('/workspaces'),

	getSettings: (workspaceId: string) =>
		api<WorkspaceSettings>(`/workspaces/${workspaceId}/settings`),

	updateSettings: (workspaceId: string, settings: { permission_mode?: PermissionMode }) =>
		api<WorkspaceSettings>(`/workspaces/${workspaceId}/settings`, {
			method: 'PUT',
			body: JSON.stringify(settings)
		})
};
