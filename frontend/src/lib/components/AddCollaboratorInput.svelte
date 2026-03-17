<script lang="ts">
	import { conversationService } from '$lib/services/conversations';

	/** Delay before hiding suggestions on blur, allowing click events to fire first. */
	const BLUR_DELAY_MS = 150;
	const DEBOUNCE_MS = 300;
	const MIN_QUERY_LEN = 2;

	interface Props {
		onAdd: (username: string) => void;
		existingUserIds?: string[];
	}

	let { onAdd, existingUserIds = [] }: Props = $props();

	let username = $state('');
	let suggestions = $state<{ id: string; username: string }[]>([]);
	let showSuggestions = $state(false);
	let debounceTimer: ReturnType<typeof setTimeout> | undefined;

	let filtered = $derived(suggestions.filter((s) => !existingUserIds.includes(s.id)));

	async function fetchSuggestions(query: string) {
		if (query.length < MIN_QUERY_LEN) {
			suggestions = [];
			return;
		}
		try {
			suggestions = await conversationService.searchUsers(query);
		} catch {
			suggestions = [];
		}
	}

	function handleInput() {
		clearTimeout(debounceTimer);
		showSuggestions = true;
		debounceTimer = setTimeout(() => {
			fetchSuggestions(username);
		}, DEBOUNCE_MS);
	}

	function handleSelect(suggestion: { id: string; username: string }) {
		onAdd(suggestion.username);
		username = '';
		suggestions = [];
		showSuggestions = false;
	}

	function handleSubmit() {
		const trimmed = username.trim();
		if (trimmed) {
			onAdd(trimmed);
			username = '';
			suggestions = [];
			showSuggestions = false;
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			e.preventDefault();
			handleSubmit();
		} else if (e.key === 'Escape') {
			showSuggestions = false;
		}
	}

	function handleFocus() {
		if (username.length >= MIN_QUERY_LEN) {
			showSuggestions = true;
		}
	}

	function handleBlur() {
		setTimeout(() => {
			showSuggestions = false;
		}, BLUR_DELAY_MS);
	}
</script>

<div class="flex gap-2">
	<div class="relative min-w-0 flex-1">
		<input
			type="text"
			bind:value={username}
			onkeydown={handleKeydown}
			oninput={handleInput}
			onfocus={handleFocus}
			onblur={handleBlur}
			placeholder="Add by username..."
			class="w-full rounded-md border border-zinc-300 bg-transparent px-3 py-1.5 text-sm text-zinc-900 outline-none placeholder:text-zinc-400 focus:border-zinc-500 dark:border-zinc-600 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400"
		/>

		{#if showSuggestions && filtered.length > 0}
			<ul
				class="absolute left-0 top-full z-50 mt-1 w-full overflow-y-auto rounded-lg border border-zinc-200 bg-white py-1 shadow-md dark:border-zinc-700 dark:bg-zinc-800"
			>
				{#each filtered as suggestion (suggestion.id)}
					<li>
						<button
							onmousedown={() => handleSelect(suggestion)}
							class="flex w-full items-center px-3 py-1.5 text-left text-sm text-zinc-700 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-zinc-700"
						>
							{suggestion.username}
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
	<button
		onclick={handleSubmit}
		disabled={!username.trim()}
		class="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-zinc-700 disabled:opacity-40 disabled:hover:bg-zinc-900 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-300 dark:disabled:hover:bg-zinc-100"
	>
		Add
	</button>
</div>
