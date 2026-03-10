import { api } from '$lib/utils/api';
import type { McpServer } from '$lib/types';

export const mcpService = {
	list: () => api<McpServer[]>('/mcp/servers'),

	create: (data: { name: string; command: string; args: unknown[]; env: Record<string, string> }) =>
		api<McpServer>('/mcp/servers', {
			method: 'POST',
			body: JSON.stringify(data)
		}),

	update: (id: string, data: Record<string, unknown>) =>
		api<McpServer>(`/mcp/servers/${id}`, {
			method: 'PATCH',
			body: JSON.stringify(data)
		}),

	remove: (id: string) => api(`/mcp/servers/${id}`, { method: 'DELETE' })
};
