import { api } from '$lib/utils/api';
import type { ConversationWithMessages } from '$lib/types';

export async function load({ params }: { params: { id: string } }) {
	const conversation = await api<ConversationWithMessages>(`/conversations/${params.id}`);
	return { conversation };
}
