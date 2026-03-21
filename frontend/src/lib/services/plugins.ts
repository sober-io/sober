import { api } from '$lib/utils/api';
import type {
	Plugin,
	PluginAuditLog,
	PluginKind,
	PluginStatus,
	ImportPluginsResult,
	ReloadPluginsResult
} from '$lib/types/plugin';

export const pluginService = {
	list: (params?: { kind?: PluginKind; status?: PluginStatus }) => {
		const searchParams = new URLSearchParams();
		if (params?.kind) searchParams.set('kind', params.kind);
		if (params?.status) searchParams.set('status', params.status);
		const query = searchParams.toString();
		return api<Plugin[]>(`/plugins${query ? `?${query}` : ''}`);
	},

	get: (id: string) => api<Plugin>(`/plugins/${id}`),

	install: (data: {
		name: string;
		kind: string;
		config: Record<string, unknown>;
		description?: string;
		version?: string;
	}) =>
		api<Plugin>('/plugins', {
			method: 'POST',
			body: JSON.stringify(data)
		}),

	import: (mcpServers: Record<string, unknown>) =>
		api<ImportPluginsResult>('/plugins/import', {
			method: 'POST',
			body: JSON.stringify({ mcpServers })
		}),

	update: (id: string, data: { enabled?: boolean; config?: Record<string, unknown> }) =>
		api<Plugin>(`/plugins/${id}`, {
			method: 'PATCH',
			body: JSON.stringify(data)
		}),

	remove: (id: string) => api<{ deleted: boolean }>(`/plugins/${id}`, { method: 'DELETE' }),

	audit: (id: string) => api<PluginAuditLog[]>(`/plugins/${id}/audit`),

	reload: () => api<ReloadPluginsResult>('/plugins/reload', { method: 'POST' })
};
