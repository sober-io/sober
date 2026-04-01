<script lang="ts">
	interface Props {
		attachmentId: string;
		alt?: string;
	}

	let { attachmentId, alt = '' }: Props = $props();

	const src = $derived(`/api/v1/attachments/${attachmentId}/content`);
</script>

<button
	type="button"
	onclick={() => window.open(src, '_blank')}
	class="inline-block cursor-pointer border-0 bg-transparent p-0"
>
	<img loading="lazy" {src} {alt} class="max-h-96 max-w-full rounded-md object-contain" />
</button>
