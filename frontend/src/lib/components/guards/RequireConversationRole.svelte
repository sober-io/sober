<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { ConversationUserRole } from '$lib/types';
	import { hasConversationRole } from '$lib/guards';

	interface Props {
		userRole: ConversationUserRole | undefined;
		minimum: ConversationUserRole;
		children: Snippet;
		fallback?: Snippet;
	}

	let { userRole, minimum, children, fallback }: Props = $props();
</script>

{#if hasConversationRole(userRole, minimum)}
	{@render children()}
{:else if fallback}
	{@render fallback()}
{/if}
