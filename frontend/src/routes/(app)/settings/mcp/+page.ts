import { api } from '$lib/utils/api';
import type { McpServer } from '$lib/types';

export async function load() {
	const servers = await api<McpServer[]>('/mcp/servers');
	return { servers };
}
