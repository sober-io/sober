<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		children: Snippet;
	}

	let { children }: Props = $props();
</script>

<div class="flex min-h-screen items-center justify-center bg-zinc-50 dark:bg-zinc-950">
	<div class="w-full max-w-md px-4">
		{@render children()}
	</div>
</div>
