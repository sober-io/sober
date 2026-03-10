<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/stores';
</script>

<div
	class="rounded-lg border border-zinc-200 bg-white p-8 text-center shadow-sm dark:border-zinc-800 dark:bg-zinc-900"
>
	<h1 class="mb-2 text-2xl font-semibold text-zinc-900 dark:text-zinc-100">
		{$page.status}
	</h1>
	<p class="mb-4 text-sm text-zinc-500 dark:text-zinc-400">
		{$page.error?.message ?? 'Something went wrong'}
	</p>
	<a href={resolve('/login')} class="text-sm text-zinc-900 underline dark:text-zinc-100">
		Back to sign in
	</a>
</div>
