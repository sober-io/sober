<script lang="ts">
	import '../app.css';
	import type { Snippet } from 'svelte';
	import { conversations } from '$lib/stores/conversations.svelte';
	import { faviconBadge } from '$lib/stores/notifications.svelte';

	interface Props {
		children: Snippet;
	}

	let { children }: Props = $props();
	const title = $derived(
		conversations.totalUnread > 0 ? `(${conversations.totalUnread}) Sõber` : 'Sõber'
	);

	$effect(() => {
		faviconBadge.update(conversations.totalUnread);
	});
</script>

<svelte:head>
	<title>{title}</title>
</svelte:head>

{@render children()}
