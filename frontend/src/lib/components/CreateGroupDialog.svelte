<script lang="ts">
	interface Props {
		open: boolean;
		onClose: () => void;
		onCreate: (title: string, members: string[]) => void;
	}

	let { open, onClose, onCreate }: Props = $props();

	let dialog: HTMLDialogElement | undefined = $state();
	let title = $state('');
	let memberInput = $state('');
	let members = $state<string[]>([]);

	const canCreate = $derived(title.trim().length > 0);

	$effect(() => {
		if (!dialog) return;
		if (open && !dialog.open) {
			dialog.showModal();
		} else if (!open && dialog.open) {
			dialog.close();
		}
	});

	// Reset form when dialog opens
	$effect(() => {
		if (open) {
			title = '';
			memberInput = '';
			members = [];
		}
	});

	const handleKeydown = (e: KeyboardEvent) => {
		if (e.key === 'Escape') {
			e.preventDefault();
			onClose();
		}
	};

	const addMember = () => {
		const username = memberInput.trim();
		if (username && !members.includes(username)) {
			members.push(username);
			memberInput = '';
		}
	};

	const removeMember = (username: string) => {
		members = members.filter((m) => m !== username);
	};

	const handleMemberKeydown = (e: KeyboardEvent) => {
		if (e.key === 'Enter') {
			e.preventDefault();
			addMember();
		}
	};

	const handleCreate = () => {
		if (!canCreate) return;
		onCreate(title.trim(), [...members]);
	};
</script>

<dialog
	bind:this={dialog}
	onkeydown={handleKeydown}
	onclose={onClose}
	class="m-auto w-full max-w-md rounded-lg border border-zinc-200 bg-white p-6 shadow-xl dark:border-zinc-700 dark:bg-zinc-900 backdrop:bg-black/50"
>
	<h2 class="mb-4 text-base font-semibold text-zinc-900 dark:text-zinc-100">Create group</h2>

	<!-- Title input -->
	<label class="mb-4 block">
		<span class="mb-1 block text-sm font-medium text-zinc-700 dark:text-zinc-300">Title</span>
		<input
			type="text"
			bind:value={title}
			placeholder="Group name"
			class="w-full rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:outline-none dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400"
		/>
	</label>

	<!-- Member input -->
	<div class="mb-4">
		<span class="mb-1 block text-sm font-medium text-zinc-700 dark:text-zinc-300">Members</span>
		<div class="flex gap-2">
			<input
				type="text"
				bind:value={memberInput}
				onkeydown={handleMemberKeydown}
				placeholder="Username"
				class="min-w-0 flex-1 rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:outline-none dark:border-zinc-600 dark:bg-zinc-800 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400"
			/>
			<button
				onclick={addMember}
				disabled={!memberInput.trim()}
				class="shrink-0 rounded-md border border-zinc-300 px-3 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-zinc-600 dark:text-zinc-300 dark:hover:bg-zinc-800"
			>
				Add
			</button>
		</div>
	</div>

	<!-- Member list -->
	{#if members.length > 0}
		<div class="mb-4 flex flex-wrap gap-2">
			{#each members as username (username)}
				<span
					class="inline-flex items-center gap-1 rounded-full bg-zinc-100 px-2.5 py-1 text-sm text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300"
				>
					{username}
					<button
						onclick={() => removeMember(username)}
						class="ml-0.5 rounded-full p-0.5 text-zinc-400 hover:text-zinc-600 dark:text-zinc-500 dark:hover:text-zinc-300"
						aria-label="Remove {username}"
					>
						<svg class="h-3 w-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="2"
								d="M6 18L18 6M6 6l12 12"
							/>
						</svg>
					</button>
				</span>
			{/each}
		</div>
	{/if}

	<!-- Actions -->
	<div class="flex justify-end gap-3">
		<button
			onclick={onClose}
			class="rounded-md border border-zinc-300 px-4 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-50 dark:border-zinc-600 dark:text-zinc-300 dark:hover:bg-zinc-800"
		>
			Cancel
		</button>
		<button
			onclick={handleCreate}
			disabled={!canCreate}
			class="rounded-md bg-zinc-900 px-4 py-2 text-sm font-medium text-white hover:bg-zinc-800 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
		>
			Create group
		</button>
	</div>
</dialog>
