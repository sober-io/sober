// Plugin domain types mirroring the unified backend plugin API.

export type PluginKind = 'mcp' | 'skill' | 'wasm';
export type PluginStatus = 'enabled' | 'disabled' | 'failed';

export interface Plugin {
	id: string;
	name: string;
	kind: PluginKind;
	version: string;
	description: string;
	status: PluginStatus;
	config: Record<string, unknown>;
	installed_at: string;
}

/** MCP-specific config shape (nested inside Plugin.config). */
export interface McpPluginConfig {
	command: string;
	args: string[];
	env: Record<string, string>;
}

/** Audit log entry for a plugin. */
export interface PluginAuditLog {
	id: string;
	plugin_id: string | null;
	plugin_name: string;
	kind: string;
	origin: string;
	stages: unknown;
	verdict: string;
	rejection_reason: string | null;
	audited_at: string;
	audited_by: string | null;
}

/** Response from POST /plugins/import. */
export interface ImportPluginsResult {
	imported_count: number;
	plugins: Plugin[];
}

/** Response from POST /plugins/reload. */
export interface ReloadPluginsResult {
	active_count: number;
}
