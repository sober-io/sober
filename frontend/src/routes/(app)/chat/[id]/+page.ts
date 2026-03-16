import { conversationService } from '$lib/services/conversations';

export const load = async ({ params }: { params: { id: string } }) => {
	const [conversation, messages] = await Promise.all([
		conversationService.get(params.id),
		conversationService.listMessages(params.id)
	]);
	return { conversation, messages };
};
