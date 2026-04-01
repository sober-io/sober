<script lang="ts">
	import SlashCommandPalette from './SlashCommandPalette.svelte';
	import AttachmentPreview from './AttachmentPreview.svelte';
	import type { SkillInfo, ContentBlock } from '$lib/types';
	import { uploads } from '$lib/stores/uploads.svelte';

	interface Props {
		onsend: (content: ContentBlock[]) => void;
		busy?: boolean;
		value?: string;
		conversationId?: string;
		onSlashCommand?: (command: string) => void;
		skills?: SkillInfo[];
	}

	let {
		onsend,
		busy = false,
		value = $bindable(''),
		conversationId,
		onSlashCommand,
		skills = []
	}: Props = $props();

	const BUILTIN_COMMANDS = ['/help', '/info', '/clear', '/reload-skills'];

	// Show palette only when typing a command prefix, not after selecting one.
	// e.g. "/" or "/hel" shows palette, but "/test-skill do something" does not.
	const showSlashCommands = $derived(value.startsWith('/') && !value.includes(' '));

	const canSend = $derived((value.trim() || uploads.hasAttachments) && !uploads.hasUploading);

	let fileInput: HTMLInputElement | undefined = $state();
	let dragOver = $state(false);

	const handleKeydown = (e: KeyboardEvent) => {
		if (e.key === 'Enter' && !e.shiftKey) {
			if (showSlashCommands) return; // let palette handle Enter
			e.preventDefault();
			submit();
		}
	};

	const submit = () => {
		const trimmed = value.trim();
		if (!trimmed && !uploads.hasAttachments) return;
		if (uploads.hasUploading) return;

		if (trimmed.startsWith('/') && !uploads.hasAttachments) {
			const cmd = trimmed.split(' ')[0];
			if (BUILTIN_COMMANDS.includes(cmd) && onSlashCommand) {
				onSlashCommand(cmd);
				value = '';
				return;
			}
			// Skill slash command — send as content blocks
			onsend([{ type: 'text', text: trimmed }]);
			value = '';
			return;
		}

		const blocks = uploads.buildContentBlocks(trimmed);
		if (blocks.length === 0) return;
		onsend(blocks);
		uploads.clear();
		value = '';
	};

	const handleSlashExecute = (command: string) => {
		if (onSlashCommand) onSlashCommand(command);
		value = '';
	};

	const handleSlashPrefill = (command: string) => {
		value = command + ' ';
	};

	const handleSlashClose = () => {
		value = '';
	};

	const openFilePicker = () => {
		fileInput?.click();
	};

	const handleFileSelect = (e: Event) => {
		const input = e.target as HTMLInputElement;
		if (input.files && input.files.length > 0 && conversationId) {
			uploads.addFiles(conversationId, input.files);
			input.value = '';
		}
	};

	const handleDrop = (e: DragEvent) => {
		e.preventDefault();
		dragOver = false;
		if (e.dataTransfer?.files && e.dataTransfer.files.length > 0 && conversationId) {
			uploads.addFiles(conversationId, e.dataTransfer.files);
		}
	};

	const handleDragOver = (e: DragEvent) => {
		e.preventDefault();
		dragOver = true;
	};

	const handleDragLeave = () => {
		dragOver = false;
	};

	const handlePaste = (e: ClipboardEvent) => {
		if (!e.clipboardData || !conversationId) return;
		const imageFiles: File[] = [];
		for (const item of e.clipboardData.items) {
			if (item.type.startsWith('image/')) {
				const file = item.getAsFile();
				if (file) imageFiles.push(file);
			}
		}
		if (imageFiles.length > 0) {
			e.preventDefault();
			uploads.addFiles(conversationId, imageFiles);
		}
	};
</script>

<div
	class={[
		'relative border-t border-zinc-200 bg-white dark:border-zinc-800 dark:bg-zinc-950',
		dragOver && 'ring-2 ring-inset ring-zinc-400 dark:ring-zinc-500'
	]}
	ondrop={handleDrop}
	ondragover={handleDragOver}
	ondragleave={handleDragLeave}
	role="presentation"
>
	{#if showSlashCommands}
		<SlashCommandPalette
			query={value}
			onExecute={handleSlashExecute}
			onPrefill={handleSlashPrefill}
			onClose={handleSlashClose}
			{skills}
		/>
	{/if}

	<AttachmentPreview attachments={uploads.attachments} onremove={uploads.removeAttachment} />

	<div class="flex gap-2 p-4">
		<input bind:this={fileInput} type="file" multiple class="hidden" onchange={handleFileSelect} />
		<button
			onclick={openFilePicker}
			class="shrink-0 rounded-md p-2 text-zinc-500 hover:bg-zinc-100 hover:text-zinc-700 dark:text-zinc-400 dark:hover:bg-zinc-800 dark:hover:text-zinc-200"
			title="Attach file"
			type="button"
		>
			<svg class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
				<path
					stroke-linecap="round"
					stroke-linejoin="round"
					stroke-width="2"
					d="M15.172 7l-6.586 6.586a2 2 0 102.828 2.828l6.414-6.586a4 4 0 00-5.656-5.656l-6.415 6.585a6 6 0 108.486 8.486L20.5 13"
				/>
			</svg>
		</button>
		<textarea
			bind:value
			onkeydown={handleKeydown}
			onpaste={handlePaste}
			placeholder="Send a message... (/ for commands)"
			rows="1"
			class="flex-1 resize-none rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400 dark:focus:ring-zinc-400"
		></textarea>
		<button
			onclick={submit}
			disabled={!canSend}
			class="rounded-md bg-zinc-900 px-4 py-2 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
		>
			{busy ? 'Queue' : 'Send'}
		</button>
	</div>
</div>
