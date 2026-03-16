<script lang="ts">
	import type { Tag } from '$lib/types';
	import { tagService } from '$lib/services/tags';

	interface Props {
		tags: Tag[];
		onAdd: (name: string) => void;
		onRemove: (tagId: string) => void;
	}

	let { tags, onAdd, onRemove }: Props = $props();
	let input = $state('');
	let showInput = $state(false);
	let allTags = $state<Tag[]>([]);
	let showSuggestions = $state(false);
	let inputEl: HTMLInputElement | undefined = $state();

	let filtered = $derived(
		allTags
			.filter((t) => !tags.some((existing) => existing.id === t.id))
			.filter((t) => t.name.toLowerCase().includes(input.toLowerCase()))
	);

	async function loadTags() {
		allTags = await tagService.list();
	}

	function handleAdd(name: string) {
		if (!name.trim()) return;
		onAdd(name.trim());
		input = '';
		showInput = false;
		showSuggestions = false;
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			e.preventDefault();
			handleAdd(input);
		} else if (e.key === 'Escape') {
			showInput = false;
			showSuggestions = false;
			input = '';
		}
	}

	function handleShowInput() {
		showInput = true;
		loadTags();
	}

	function handleInputFocus() {
		showSuggestions = true;
	}

	function handleInputBlur() {
		// Delay so click on suggestion fires first
		setTimeout(() => {
			showSuggestions = false;
		}, 150);
	}

	// Focus input when it becomes visible
	$effect(() => {
		if (showInput && inputEl) {
			inputEl.focus();
		}
	});
</script>

<div class="flex flex-wrap items-center gap-1.5">
	{#each tags as tag (tag.id)}
		<span
			class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium"
			style="background-color: color-mix(in srgb, {tag.color} 15%, transparent); color: {tag.color}; border: 1px solid color-mix(in srgb, {tag.color} 30%, transparent);"
		>
			{tag.name}
			<button
				onclick={() => onRemove(tag.id)}
				class="ml-0.5 rounded-full opacity-60 hover:opacity-100 focus:outline-none"
				aria-label="Remove tag {tag.name}"
			>
				<svg
					xmlns="http://www.w3.org/2000/svg"
					viewBox="0 0 12 12"
					fill="currentColor"
					class="h-2.5 w-2.5"
				>
					<path
						d="M9.354 2.646a.5.5 0 0 0-.708 0L6 5.293 3.354 2.646a.5.5 0 0 0-.708.708L5.293 6 2.646 8.646a.5.5 0 0 0 .708.708L6 6.707l2.646 2.647a.5.5 0 0 0 .708-.708L6.707 6l2.647-2.646a.5.5 0 0 0 0-.708z"
					/>
				</svg>
			</button>
		</span>
	{/each}

	{#if showInput}
		<div class="relative">
			<input
				bind:this={inputEl}
				bind:value={input}
				onkeydown={handleKeydown}
				onfocus={handleInputFocus}
				onblur={handleInputBlur}
				oninput={() => (showSuggestions = true)}
				placeholder="Tag name…"
				class="w-28 rounded-full border border-zinc-300 bg-transparent px-2 py-0.5 text-xs text-zinc-700 outline-none placeholder:text-zinc-400 focus:border-zinc-500 dark:border-zinc-600 dark:text-zinc-300 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400"
			/>

			{#if showSuggestions && filtered.length > 0}
				<ul
					class="absolute left-0 top-full z-50 mt-1 max-h-40 min-w-[120px] overflow-y-auto rounded-lg border border-zinc-200 bg-white py-1 shadow-md dark:border-zinc-700 dark:bg-zinc-800"
				>
					{#each filtered as suggestion (suggestion.id)}
						<li>
							<button
								onmousedown={() => handleAdd(suggestion.name)}
								class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs text-zinc-700 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-zinc-700"
							>
								<span
									class="inline-block h-2 w-2 shrink-0 rounded-full"
									style="background-color: {suggestion.color};"
								></span>
								{suggestion.name}
							</button>
						</li>
					{/each}
				</ul>
			{/if}
		</div>
	{:else}
		<button
			onclick={handleShowInput}
			class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs text-zinc-400 hover:text-zinc-600 dark:text-zinc-500 dark:hover:text-zinc-300"
		>
			<svg
				xmlns="http://www.w3.org/2000/svg"
				viewBox="0 0 12 12"
				fill="currentColor"
				class="h-2.5 w-2.5"
			>
				<path
					d="M6 1a.5.5 0 0 1 .5.5v4h4a.5.5 0 0 1 0 1h-4v4a.5.5 0 0 1-1 0v-4h-4a.5.5 0 0 1 0-1h4v-4A.5.5 0 0 1 6 1z"
				/>
			</svg>
			Add tag
		</button>
	{/if}
</div>
