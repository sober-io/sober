<script lang="ts">
	import { auth } from '$lib/stores/auth.svelte';
</script>

<div class="flex flex-1 items-center justify-center">
	<div class="text-center">
		<h1 class="mb-2 text-2xl font-semibold text-zinc-900 dark:text-zinc-100">
			Hello, {auth.user?.username ?? 'there'}
		</h1>
		<p class="text-sm text-zinc-500 dark:text-zinc-400">
			Select a conversation or start a new one.
		</p>
	</div>
</div>
