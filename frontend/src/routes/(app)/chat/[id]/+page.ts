import { conversationService } from '$lib/services/conversations';

export const load = async ({ params }: { params: { id: string } }) => {
	const conversation = await conversationService.get(params.id);
	return { conversation };
};
