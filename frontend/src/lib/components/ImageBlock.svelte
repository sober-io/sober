<script lang="ts">
	interface Props {
		attachmentId: string;
		alt?: string;
	}

	let { attachmentId, alt = '' }: Props = $props();

	const src = $derived(`/api/v1/attachments/${attachmentId}/content`);
</script>

<a href={src} target="_blank" rel="noopener noreferrer" class="inline-block">
	<img loading="lazy" {src} {alt} class="max-h-96 max-w-full rounded-md object-contain" />
</a>
