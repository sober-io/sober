import { SvelteMap } from 'svelte/reactivity';
import type { ContentBlock, ConversationAttachment } from '$lib/types';

export type UploadStatus = 'uploading' | 'ready' | 'failed';

export interface AttachmentState {
	id: string;
	file: File;
	status: UploadStatus;
	previewUrl?: string;
	attachment?: ConversationAttachment;
	error?: string;
}

export const uploads = (() => {
	const attachments = new SvelteMap<string, AttachmentState>();

	async function addFiles(conversationId: string, files: FileList | File[]) {
		for (const file of files) {
			const tempId = crypto.randomUUID();
			const previewUrl = file.type.startsWith('image/') ? URL.createObjectURL(file) : undefined;

			attachments.set(tempId, {
				id: tempId,
				file,
				status: 'uploading',
				previewUrl
			});

			try {
				const formData = new FormData();
				formData.append('file', file);
				const res = await fetch(`/api/v1/conversations/${conversationId}/attachments`, {
					method: 'POST',
					body: formData
				});
				if (!res.ok) {
					throw new Error(`Upload failed: ${res.status}`);
				}
				const { data } = await res.json();
				const current = attachments.get(tempId);
				if (current) {
					attachments.set(tempId, { ...current, status: 'ready', attachment: data });
				}
			} catch (e) {
				const current = attachments.get(tempId);
				if (current) {
					attachments.set(tempId, {
						...current,
						status: 'failed',
						error: e instanceof Error ? e.message : 'Upload failed'
					});
				}
			}
		}
	}

	function removeAttachment(id: string) {
		const state = attachments.get(id);
		if (state?.previewUrl) {
			URL.revokeObjectURL(state.previewUrl);
		}
		attachments.delete(id);
	}

	function buildContentBlocks(text: string): ContentBlock[] {
		const blocks: ContentBlock[] = [];
		if (text.trim()) {
			blocks.push({ type: 'text', text });
		}
		for (const [, state] of attachments) {
			if (state.status !== 'ready' || !state.attachment) continue;
			const kind = state.attachment.kind;
			const id = state.attachment.id;
			if (kind === 'image') {
				blocks.push({ type: 'image', conversation_attachment_id: id });
			} else if (kind === 'audio') {
				blocks.push({ type: 'audio', conversation_attachment_id: id });
			} else if (kind === 'video') {
				blocks.push({ type: 'video', conversation_attachment_id: id });
			} else {
				blocks.push({ type: 'file', conversation_attachment_id: id });
			}
		}
		return blocks;
	}

	function clear() {
		for (const [, state] of attachments) {
			if (state.previewUrl) {
				URL.revokeObjectURL(state.previewUrl);
			}
		}
		attachments.clear();
	}

	return {
		get attachments() {
			return attachments;
		},
		get hasUploading() {
			for (const [, s] of attachments) {
				if (s.status === 'uploading') return true;
			}
			return false;
		},
		get hasAttachments() {
			return attachments.size > 0;
		},
		addFiles,
		removeAttachment,
		buildContentBlocks,
		clear
	};
})();
