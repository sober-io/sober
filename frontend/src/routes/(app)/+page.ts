import { conversationService } from '$lib/services/conversations';
import type { PageLoad } from './$types';

export const load: PageLoad = async () => {
	const [all, inbox] = await Promise.all([
		conversationService.list(),
		conversationService.getInbox()
	]);
	return { conversations: all, inbox };
};
