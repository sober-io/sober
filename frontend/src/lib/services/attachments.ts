import type { ConversationAttachment } from '$lib/types';

/** Upload a file attachment to a conversation. */
export async function uploadAttachment(
	conversationId: string,
	file: File
): Promise<ConversationAttachment> {
	const formData = new FormData();
	formData.append('file', file);

	const res = await fetch(`/api/v1/conversations/${conversationId}/attachments`, {
		method: 'POST',
		body: formData
	});

	if (!res.ok) {
		const error = await res.json().catch(() => ({ error: { message: 'Upload failed' } }));
		throw new Error(error.error?.message || `Upload failed: ${res.status}`);
	}

	const { data } = await res.json();
	return data;
}

/** Returns the URL for serving an attachment's content. */
export function getAttachmentUrl(attachmentId: string): string {
	return `/api/v1/attachments/${attachmentId}/content`;
}
