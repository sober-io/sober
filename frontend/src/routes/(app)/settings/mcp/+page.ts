import { mcpService } from '$lib/services/mcp';

export const load = async () => {
	const servers = await mcpService.list();
	return { servers };
};
