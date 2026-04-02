<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { SystemRole } from '$lib/types';
	import { hasRole } from '$lib/guards';

	interface Props {
		role: SystemRole;
		children: Snippet;
		fallback?: Snippet;
	}

	let { role, children, fallback }: Props = $props();
</script>

{#if hasRole(role)}
	{@render children()}
{:else if fallback}
	{@render fallback()}
{/if}
