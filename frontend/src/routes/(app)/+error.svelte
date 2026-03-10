<script lang="ts">
	import { page } from '$app/stores';
</script>

<div class="flex flex-1 items-center justify-center">
	<div class="text-center">
		<h1 class="mb-2 text-2xl font-semibold text-zinc-900 dark:text-zinc-100">
			{$page.status}
		</h1>
		<p class="mb-4 text-sm text-zinc-500 dark:text-zinc-400">
			{$page.error?.message ?? 'Something went wrong'}
		</p>
		<a
			href="/"
			class="rounded-md bg-zinc-900 px-4 py-2 text-sm font-medium text-white hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
		>
			Go home
		</a>
	</div>
</div>
