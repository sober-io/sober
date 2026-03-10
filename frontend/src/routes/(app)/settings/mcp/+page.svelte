<script lang="ts">
	import type { McpServer } from '$lib/types';
	import { ApiError } from '$lib/utils/api';
	import { mcpService } from '$lib/services/mcp';

	let { data }: { data: { servers: McpServer[] } } = $props();

	// eslint-disable-next-line svelte/prefer-writable-derived -- servers is mutated by CRUD operations
	let servers = $state<McpServer[]>([]);

	$effect(() => {
		servers = data.servers;
	});
	let error = $state<string | null>(null);

	// Form state for add/edit
	let editing = $state<string | null>(null); // server id or 'new'
	let formName = $state('');
	let formCommand = $state('');
	let formArgs = $state('');
	let formEnv = $state('');
	let submitting = $state(false);

	let deleteConfirm = $state<string | null>(null);

	const startAdd = () => {
		editing = 'new';
		formName = '';
		formCommand = '';
		formArgs = '';
		formEnv = '';
	};

	const startEdit = (server: McpServer) => {
		editing = server.id;
		formName = server.name;
		formCommand = server.command;
		formArgs = JSON.stringify(server.args);
		formEnv = JSON.stringify(server.env);
	};

	const cancelEdit = () => {
		editing = null;
		error = null;
	};

	const saveServer = async () => {
		error = null;
		submitting = true;

		let args: unknown[];
		let env: Record<string, string>;
		try {
			args = formArgs ? JSON.parse(formArgs) : [];
			env = formEnv ? JSON.parse(formEnv) : {};
		} catch {
			error = 'Invalid JSON in args or env fields';
			submitting = false;
			return;
		}

		try {
			if (editing === 'new') {
				const server = await mcpService.create({
					name: formName,
					command: formCommand,
					args,
					env
				});
				servers = [...servers, server];
			} else if (editing) {
				const server = await mcpService.update(editing, {
					name: formName,
					command: formCommand,
					args,
					env
				});
				servers = servers.map((s) => (s.id === editing ? server : s));
			}
			editing = null;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'An unexpected error occurred';
		} finally {
			submitting = false;
		}
	};

	const toggleEnabled = async (server: McpServer) => {
		try {
			const updated = await mcpService.update(server.id, { enabled: !server.enabled });
			servers = servers.map((s) => (s.id === server.id ? updated : s));
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to toggle server';
		}
	};

	const argsPlaceholder = '["--flag", "value"]';

	const deleteServer = async (id: string) => {
		try {
			await mcpService.remove(id);
			servers = servers.filter((s) => s.id !== id);
			deleteConfirm = null;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to delete server';
		}
	};
</script>

<div class="mx-auto max-w-2xl p-6">
	<div class="mb-6 flex items-center justify-between">
		<h1 class="text-xl font-semibold text-zinc-900 dark:text-zinc-100">MCP Servers</h1>
		<button
			onclick={startAdd}
			disabled={editing !== null}
			class="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
		>
			Add server
		</button>
	</div>

	{#if error}
		<div
			class="mb-4 rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-950 dark:text-red-300"
		>
			{error}
		</div>
	{/if}

	<!-- Add/Edit form -->
	{#if editing !== null}
		<div class="mb-6 rounded-lg border border-zinc-200 p-4 dark:border-zinc-700">
			<h2 class="mb-3 text-sm font-medium text-zinc-900 dark:text-zinc-100">
				{editing === 'new' ? 'Add server' : 'Edit server'}
			</h2>
			<div class="space-y-3">
				<div>
					<label
						for="mcp-name"
						class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400">Name</label
					>
					<input
						id="mcp-name"
						type="text"
						bind:value={formName}
						class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
					/>
				</div>
				<div>
					<label
						for="mcp-command"
						class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400">Command</label
					>
					<input
						id="mcp-command"
						type="text"
						bind:value={formCommand}
						class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm font-mono text-zinc-900 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
					/>
				</div>
				<div>
					<label
						for="mcp-args"
						class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
						>Args (JSON array)</label
					>
					<input
						id="mcp-args"
						type="text"
						bind:value={formArgs}
						placeholder={argsPlaceholder}
						class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm font-mono text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
					/>
				</div>
				<div>
					<label
						for="mcp-env"
						class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
						>Env (JSON object)</label
					>
					<input
						id="mcp-env"
						type="text"
						bind:value={formEnv}
						placeholder={'{"KEY": "value"}'}
						class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm font-mono text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
					/>
				</div>
				<div class="flex gap-2">
					<button
						onclick={saveServer}
						disabled={submitting || !formName || !formCommand}
						class="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
					>
						{submitting ? 'Saving...' : 'Save'}
					</button>
					<button
						onclick={cancelEdit}
						class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
					>
						Cancel
					</button>
				</div>
			</div>
		</div>
	{/if}

	<!-- Server list -->
	<div class="space-y-2">
		{#each servers as server (server.id)}
			<div
				class="flex items-center justify-between rounded-lg border border-zinc-200 px-4 py-3 dark:border-zinc-700"
			>
				<div class="min-w-0 flex-1">
					<div class="flex items-center gap-2">
						<span class="text-sm font-medium text-zinc-900 dark:text-zinc-100">{server.name}</span>
						<span
							class={[
								'inline-block h-2 w-2 rounded-full',
								server.enabled ? 'bg-green-500' : 'bg-zinc-300 dark:bg-zinc-600'
							]}
						></span>
					</div>
					<div class="mt-0.5 truncate font-mono text-xs text-zinc-500 dark:text-zinc-400">
						{server.command}
						{JSON.stringify(server.args)}
					</div>
				</div>
				<div class="ml-4 flex items-center gap-1">
					<button
						onclick={() => toggleEnabled(server)}
						class="rounded px-2 py-1 text-xs text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800"
					>
						{server.enabled ? 'Disable' : 'Enable'}
					</button>
					<button
						onclick={() => startEdit(server)}
						disabled={editing !== null}
						class="rounded px-2 py-1 text-xs text-zinc-600 hover:bg-zinc-100 disabled:opacity-50 dark:text-zinc-400 dark:hover:bg-zinc-800"
					>
						Edit
					</button>
					{#if deleteConfirm === server.id}
						<button
							onclick={() => deleteServer(server.id)}
							class="rounded px-2 py-1 text-xs text-red-600 hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-950"
						>
							Confirm
						</button>
						<button
							onclick={() => (deleteConfirm = null)}
							class="rounded px-2 py-1 text-xs text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800"
						>
							Cancel
						</button>
					{:else}
						<button
							onclick={() => (deleteConfirm = server.id)}
							disabled={editing !== null}
							class="rounded px-2 py-1 text-xs text-red-600 hover:bg-red-50 disabled:opacity-50 dark:text-red-400 dark:hover:bg-red-950"
						>
							Delete
						</button>
					{/if}
				</div>
			</div>
		{/each}

		{#if servers.length === 0}
			<p class="py-8 text-center text-sm text-zinc-400 dark:text-zinc-500">
				No MCP servers configured. Add one to get started.
			</p>
		{/if}
	</div>
</div>
