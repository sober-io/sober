<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		title: string;
		description?: string;
		danger?: boolean;
		children: Snippet;
	}

	let { title, description, danger = false, children }: Props = $props();
</script>

<section class={['space-y-3 py-4', danger && 'border-l-2 border-red-500 pl-4 dark:border-red-400']}>
	<div>
		<h3
			class={[
				'text-sm font-medium',
				danger ? 'text-red-700 dark:text-red-400' : 'text-zinc-900 dark:text-zinc-100'
			]}
		>
			{title}
		</h3>
		{#if description}
			<p class="mt-0.5 text-xs text-zinc-500 dark:text-zinc-400">{description}</p>
		{/if}
	</div>
	{@render children()}
</section>
