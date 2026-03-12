import { api } from '$lib/utils/api';
import type { PermissionMode, WorkspaceSettings } from '$lib/types';

export const workspaceService = {
	getSettings: (workspaceId: string) =>
		api<WorkspaceSettings>(`/workspaces/${workspaceId}/settings`),

	updateSettings: (workspaceId: string, settings: { permission_mode?: PermissionMode }) =>
		api<WorkspaceSettings>(`/workspaces/${workspaceId}/settings`, {
			method: 'PUT',
			body: JSON.stringify(settings)
		})
};
