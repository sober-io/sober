<!-- TODO: Role-based visibility for plugins page.
     - Non-admin users should only see plugins they own or that are in their scope.
     - Enable/disable/scope-change actions should be restricted by ownership and role.
     - System-scoped plugins should be read-only for non-admin users.
     - Consider filtering plugin list server-side based on caller's roles/scopes. -->
<script lang="ts">
	import type { Plugin, PluginKind, PluginAuditLog, McpPluginConfig } from '$lib/types/plugin';
	import type { ToolInfo } from '$lib/types';
	import { ApiError } from '$lib/utils/api';
	import { pluginService } from '$lib/services/plugins';
	import { toolService } from '$lib/services/tools';

	type KindFilter = 'all' | PluginKind;

	// Plugin list state
	let plugins = $state<Plugin[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let activeFilter = $state<KindFilter>('all');

	// Install form state
	let showInstallForm = $state(false);
	let installName = $state('');
	let installCommand = $state('');
	let installArgs = $state('');
	let installEnv = $state('');
	let installDescription = $state('');
	let installSubmitting = $state(false);

	// Import state
	let showImportForm = $state(false);
	let importJson = $state('');
	let importSubmitting = $state(false);

	// Audit state
	let auditPluginId = $state<string | null>(null);
	let auditLogs = $state<PluginAuditLog[]>([]);
	let auditLoading = $state(false);

	// Tool names per plugin
	let allTools = $state<ToolInfo[]>([]);
	let pluginToolNames = $derived(
		allTools.reduce<Record<string, ToolInfo[]>>((acc, t) => {
			if (t.plugin_id) {
				(acc[t.plugin_id] ??= []).push(t);
			}
			return acc;
		}, {})
	);

	// Expanded plugin details
	let expandedId = $state<string | null>(null);

	// Delete confirm
	let deleteConfirmId = $state<string | null>(null);

	let filteredPlugins = $derived(
		activeFilter === 'all' ? plugins : plugins.filter((p) => p.kind === activeFilter)
	);

	const filters: { label: string; value: KindFilter }[] = [
		{ label: 'All', value: 'all' },
		{ label: 'MCP', value: 'mcp' },
		{ label: 'Skills', value: 'skill' },
		{ label: 'WASM', value: 'wasm' }
	];

	const kindBadgeClass: Record<PluginKind, string> = {
		mcp: 'bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200',
		skill: 'bg-purple-100 text-purple-800 dark:bg-purple-900 dark:text-purple-200',
		wasm: 'bg-amber-100 text-amber-800 dark:bg-amber-900 dark:text-amber-200'
	};

	const statusBadgeClass: Record<string, string> = {
		enabled: 'bg-emerald-100 text-emerald-800 dark:bg-emerald-900 dark:text-emerald-200',
		disabled: 'bg-zinc-100 text-zinc-800 dark:bg-zinc-700 dark:text-zinc-300',
		failed: 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200'
	};

	// Load plugins on mount
	$effect(() => {
		loadPlugins();
	});

	async function loadPlugins() {
		loading = true;
		error = null;
		try {
			[plugins, allTools] = await Promise.all([pluginService.list(), toolService.list()]);
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to load plugins';
		} finally {
			loading = false;
		}
	}

	async function reloadPlugins() {
		error = null;
		try {
			await pluginService.reload();
			await loadPlugins();
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to reload plugins';
		}
	}

	async function togglePlugin(plugin: Plugin) {
		error = null;
		try {
			const updated = await pluginService.update(plugin.id, {
				enabled: plugin.status !== 'enabled'
			});
			plugins = plugins.map((p) => (p.id === plugin.id ? updated : p));
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to update plugin';
		}
	}

	async function deletePlugin(id: string) {
		error = null;
		try {
			await pluginService.remove(id);
			plugins = plugins.filter((p) => p.id !== id);
			deleteConfirmId = null;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to delete plugin';
		}
	}

	async function installPlugin() {
		error = null;
		installSubmitting = true;

		let args: string[];
		let env: Record<string, string>;
		try {
			args = installArgs
				? installArgs
						.split(',')
						.map((a) => a.trim())
						.filter(Boolean)
				: [];
			env = installEnv
				? Object.fromEntries(
						installEnv
							.split('\n')
							.map((line) => line.trim())
							.filter(Boolean)
							.map((line) => {
								const idx = line.indexOf('=');
								return idx > 0 ? [line.slice(0, idx), line.slice(idx + 1)] : null;
							})
							.filter((entry): entry is [string, string] => entry !== null)
					)
				: {};
		} catch {
			error = 'Invalid args or env format';
			installSubmitting = false;
			return;
		}

		try {
			const plugin = await pluginService.install({
				name: installName,
				kind: 'mcp',
				config: { command: installCommand, args, env },
				description: installDescription || undefined
			});
			plugins = [...plugins, plugin];
			showInstallForm = false;
			installName = '';
			installCommand = '';
			installArgs = '';
			installEnv = '';
			installDescription = '';
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to install plugin';
		} finally {
			installSubmitting = false;
		}
	}

	async function importConfig() {
		error = null;
		importSubmitting = true;

		let parsed: Record<string, unknown>;
		try {
			const raw = JSON.parse(importJson);
			parsed = raw.mcpServers ?? raw;
		} catch {
			error = 'Invalid JSON';
			importSubmitting = false;
			return;
		}

		try {
			const result = await pluginService.import(parsed);
			plugins = [...plugins, ...result.plugins];
			showImportForm = false;
			importJson = '';
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to import plugins';
		} finally {
			importSubmitting = false;
		}
	}

	async function viewAuditLog(pluginId: string) {
		if (auditPluginId === pluginId) {
			auditPluginId = null;
			return;
		}
		auditPluginId = pluginId;
		auditLoading = true;
		auditLogs = [];
		try {
			auditLogs = await pluginService.audit(pluginId);
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to load audit log';
		} finally {
			auditLoading = false;
		}
	}

	function getMcpConfig(plugin: Plugin): McpPluginConfig | null {
		if (plugin.kind !== 'mcp' || !plugin.config) return null;
		const c = plugin.config as Record<string, unknown>;
		if (typeof c.command !== 'string') return null;
		return c as unknown as McpPluginConfig;
	}

	async function changeScope(plugin: Plugin, newScope: string) {
		error = null;
		try {
			const updated = await pluginService.update(plugin.id, { scope: newScope });
			plugins = plugins.map((p) => (p.id === plugin.id ? updated : p));
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to update scope';
		}
	}
</script>

<!-- Header -->
<div class="mb-6 flex items-center justify-between">
	<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">Plugins</h2>
	<div class="flex items-center gap-2">
		<button
			onclick={() => {
				showInstallForm = !showInstallForm;
				showImportForm = false;
			}}
			class="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
		>
			Add MCP
		</button>
		<button
			onclick={() => {
				showImportForm = !showImportForm;
				showInstallForm = false;
			}}
			class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
		>
			Import
		</button>
		<button
			onclick={reloadPlugins}
			class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
		>
			Reload
		</button>
	</div>
</div>

{#if error}
	<div class="mb-4 rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-950 dark:text-red-300">
		{error}
		<button onclick={() => (error = null)} class="ml-2 font-medium underline hover:no-underline">
			Dismiss
		</button>
	</div>
{/if}

<!-- Filter tabs -->
<div class="mb-4 flex gap-1 rounded-lg bg-zinc-100 p-1 dark:bg-zinc-800">
	{#each filters as filter (filter.value)}
		<button
			onclick={() => (activeFilter = filter.value)}
			class={[
				'rounded-md px-3 py-1.5 text-sm font-medium transition-colors',
				activeFilter === filter.value
					? 'bg-white text-zinc-900 shadow-sm dark:bg-zinc-700 dark:text-zinc-100'
					: 'text-zinc-600 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-zinc-200'
			]}
		>
			{filter.label}
		</button>
	{/each}
</div>

<!-- Install MCP form -->
{#if showInstallForm}
	<div class="mb-6 rounded-lg border border-zinc-200 p-4 dark:border-zinc-700">
		<h2 class="mb-3 text-sm font-medium text-zinc-900 dark:text-zinc-100">Add MCP Server</h2>
		<div class="space-y-3">
			<div>
				<label
					for="plugin-name"
					class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400">Name</label
				>
				<input
					id="plugin-name"
					type="text"
					bind:value={installName}
					placeholder="my-server"
					class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
				/>
			</div>
			<div>
				<label
					for="plugin-command"
					class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400">Command</label
				>
				<input
					id="plugin-command"
					type="text"
					bind:value={installCommand}
					placeholder="npx"
					class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm font-mono text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
				/>
			</div>
			<div>
				<label
					for="plugin-args"
					class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
					>Args (comma-separated)</label
				>
				<input
					id="plugin-args"
					type="text"
					bind:value={installArgs}
					placeholder="-y, @modelcontextprotocol/server-github"
					class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm font-mono text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
				/>
			</div>
			<div>
				<label
					for="plugin-env"
					class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
					>Env (one per line: KEY=value)</label
				>
				<textarea
					id="plugin-env"
					bind:value={installEnv}
					placeholder="GITHUB_TOKEN=ghp_..."
					rows="2"
					class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm font-mono text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
				></textarea>
			</div>
			<div>
				<label
					for="plugin-description"
					class="mb-1 block text-xs font-medium text-zinc-600 dark:text-zinc-400"
					>Description (optional)</label
				>
				<input
					id="plugin-description"
					type="text"
					bind:value={installDescription}
					class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
				/>
			</div>
			<div class="flex gap-2">
				<button
					onclick={installPlugin}
					disabled={installSubmitting || !installName || !installCommand}
					class="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
				>
					{installSubmitting ? 'Installing...' : 'Install'}
				</button>
				<button
					onclick={() => (showInstallForm = false)}
					class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
				>
					Cancel
				</button>
			</div>
		</div>
	</div>
{/if}

<!-- Import form -->
{#if showImportForm}
	<div class="mb-6 rounded-lg border border-zinc-200 p-4 dark:border-zinc-700">
		<h2 class="mb-3 text-sm font-medium text-zinc-900 dark:text-zinc-100">
			Import .mcp.json Config
		</h2>
		<p class="mb-2 text-xs text-zinc-500 dark:text-zinc-400">
			Paste the contents of your <code class="rounded bg-zinc-100 px-1 dark:bg-zinc-800"
				>.mcp.json</code
			>
			file or just the <code class="rounded bg-zinc-100 px-1 dark:bg-zinc-800">mcpServers</code> object.
		</p>
		<textarea
			bind:value={importJson}
			rows="6"
			placeholder={'{\n  "mcpServers": {\n    "server-name": {\n      "command": "npx",\n      "args": ["-y", "@example/server"]\n    }\n  }\n}'}
			class="mb-3 w-full rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm font-mono text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-zinc-400"
		></textarea>
		<div class="flex gap-2">
			<button
				onclick={importConfig}
				disabled={importSubmitting || !importJson.trim()}
				class="rounded-md bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
			>
				{importSubmitting ? 'Importing...' : 'Import'}
			</button>
			<button
				onclick={() => (showImportForm = false)}
				class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
			>
				Cancel
			</button>
		</div>
	</div>
{/if}

<!-- Plugin list -->
{#if loading}
	<p class="py-8 text-center text-sm text-zinc-400 dark:text-zinc-500">Loading plugins...</p>
{:else if filteredPlugins.length === 0}
	<p class="py-8 text-center text-sm text-zinc-400 dark:text-zinc-500">
		{plugins.length === 0 ? 'No plugins installed.' : 'No plugins match this filter.'}
	</p>
{:else}
	<div class="space-y-2">
		{#each filteredPlugins as plugin (plugin.id)}
			{@const tools = pluginToolNames[plugin.id] ?? []}
			<div class="rounded-lg border border-zinc-200 dark:border-zinc-700">
				<!-- Main row -->
				<div class="flex items-center justify-between px-4 py-3">
					<div class="min-w-0 flex-1">
						<div class="flex items-center gap-2">
							<button
								onclick={() => (expandedId = expandedId === plugin.id ? null : plugin.id)}
								class="text-sm font-medium text-zinc-900 hover:underline dark:text-zinc-100"
							>
								{plugin.name}
							</button>
							{#if plugin.version}
								<span class="text-xs text-zinc-400 dark:text-zinc-500">v{plugin.version}</span>
							{/if}
							<select
								value={plugin.scope}
								onchange={(e) => changeScope(plugin, e.currentTarget.value)}
								class="rounded border border-zinc-300 bg-transparent px-1 py-0 text-xs text-zinc-500 dark:border-zinc-700 dark:text-zinc-400"
							>
								{#if plugin.scope === 'workspace'}
									<option value="workspace">workspace</option>
								{/if}
								<option value="user">user</option>
								<option value="system">system</option>
							</select>
							<span
								class={[
									'inline-flex rounded-full px-2 py-0.5 text-xs font-medium',
									kindBadgeClass[plugin.kind]
								]}
							>
								{plugin.kind}
							</span>
							<span
								class={[
									'inline-flex rounded-full px-2 py-0.5 text-xs font-medium',
									statusBadgeClass[plugin.status] ?? statusBadgeClass.disabled
								]}
							>
								{plugin.status}
							</span>
						</div>
						{#if plugin.description}
							<p class="mt-0.5 truncate text-xs text-zinc-500 dark:text-zinc-400">
								{plugin.description}
							</p>
						{/if}
						{#if tools.length > 0}
							<div class="mt-1 flex flex-wrap gap-1">
								{#each tools as tool (tool.name)}
									<span
										class="inline-flex rounded bg-zinc-100 px-1.5 py-0.5 font-mono text-xs text-zinc-600 dark:bg-zinc-800 dark:text-zinc-400"
									>
										{tool.name}
									</span>
								{/each}
							</div>
						{/if}
					</div>
					<div class="ml-4 flex items-center gap-1">
						<!-- Toggle enable/disable -->
						<button
							onclick={() => togglePlugin(plugin)}
							class="rounded px-2 py-1 text-xs text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800"
						>
							{plugin.status === 'enabled' ? 'Disable' : 'Enable'}
						</button>
						<!-- Audit log -->
						<button
							onclick={() => viewAuditLog(plugin.id)}
							class="rounded px-2 py-1 text-xs text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800"
						>
							Audit
						</button>
						<!-- Delete -->
						{#if deleteConfirmId === plugin.id}
							<button
								onclick={() => deletePlugin(plugin.id)}
								class="rounded px-2 py-1 text-xs text-red-600 hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-950"
							>
								Confirm
							</button>
							<button
								onclick={() => (deleteConfirmId = null)}
								class="rounded px-2 py-1 text-xs text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800"
							>
								Cancel
							</button>
						{:else}
							<button
								onclick={() => (deleteConfirmId = plugin.id)}
								class="rounded px-2 py-1 text-xs text-red-600 hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-950"
							>
								Delete
							</button>
						{/if}
					</div>
				</div>

				<!-- Expanded details -->
				{#if expandedId === plugin.id}
					{@const mcpConfig = getMcpConfig(plugin)}
					<div
						class="border-t border-zinc-200 bg-zinc-50 px-4 py-3 dark:border-zinc-700 dark:bg-zinc-800/50"
					>
						{#if mcpConfig}
							<div class="space-y-1 text-xs">
								<div>
									<span class="font-medium text-zinc-600 dark:text-zinc-400">Command:</span>
									<code class="ml-1 text-zinc-900 dark:text-zinc-100">{mcpConfig.command}</code>
								</div>
								{#if mcpConfig.args?.length}
									<div>
										<span class="font-medium text-zinc-600 dark:text-zinc-400">Args:</span>
										<code class="ml-1 text-zinc-900 dark:text-zinc-100"
											>{JSON.stringify(mcpConfig.args)}</code
										>
									</div>
								{/if}
								{#if mcpConfig.env && Object.keys(mcpConfig.env).length > 0}
									<div>
										<span class="font-medium text-zinc-600 dark:text-zinc-400">Env:</span>
										{#each Object.entries(mcpConfig.env) as [key, value] (key)}
											<div class="ml-4">
												<code class="text-zinc-900 dark:text-zinc-100">{key}={value}</code>
											</div>
										{/each}
									</div>
								{/if}
							</div>
						{:else}
							<pre class="overflow-x-auto text-xs text-zinc-700 dark:text-zinc-300">{JSON.stringify(
									plugin.config,
									null,
									2
								)}</pre>
						{/if}
						<div class="mt-2 text-xs text-zinc-400 dark:text-zinc-500">
							Installed: {new Date(plugin.installed_at).toLocaleString()}
						</div>
					</div>
				{/if}

				<!-- Audit log panel -->
				{#if auditPluginId === plugin.id}
					<div
						class="border-t border-zinc-200 bg-zinc-50 px-4 py-3 dark:border-zinc-700 dark:bg-zinc-800/50"
					>
						<h3 class="mb-2 text-xs font-medium text-zinc-700 dark:text-zinc-300">Audit Log</h3>
						{#if auditLoading}
							<p class="text-xs text-zinc-400">Loading...</p>
						{:else if auditLogs.length === 0}
							<p class="text-xs text-zinc-400">No audit entries.</p>
						{:else}
							<div class="space-y-2">
								{#each auditLogs as log (log.id)}
									<div
										class="rounded border border-zinc-200 bg-white p-2 text-xs dark:border-zinc-700 dark:bg-zinc-800"
									>
										<div class="flex items-center gap-2">
											<span
												class={[
													'rounded-full px-1.5 py-0.5 font-medium',
													log.verdict === 'approved'
														? 'bg-emerald-100 text-emerald-800 dark:bg-emerald-900 dark:text-emerald-200'
														: log.verdict === 'rejected'
															? 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200'
															: 'bg-zinc-100 text-zinc-800 dark:bg-zinc-700 dark:text-zinc-300'
												]}
											>
												{log.verdict}
											</span>
											<span class="text-zinc-500 dark:text-zinc-400">
												{log.origin}
											</span>
											<span class="text-zinc-400 dark:text-zinc-500">
												{new Date(log.audited_at).toLocaleString()}
											</span>
										</div>
										{#if log.rejection_reason}
											<p class="mt-1 text-red-600 dark:text-red-400">
												{log.rejection_reason}
											</p>
										{/if}
									</div>
								{/each}
							</div>
						{/if}
					</div>
				{/if}
			</div>
		{/each}
	</div>
{/if}
