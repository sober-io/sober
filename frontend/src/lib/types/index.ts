// Domain types mirroring backend API response shapes

export type MessageSource = 'web' | 'gateway' | 'scheduler' | 'cli' | 'replica' | 'admin';

export interface User {
	id: string;
	email: string;
	username: string;
	status: string;
	roles: SystemRole[];
}

export type SystemRole = 'user' | 'admin';
export type ConversationKind = 'direct' | 'group' | 'inbox';
export type ConversationUserRole = 'owner' | 'admin' | 'member';
export type AgentMode = 'always' | 'mention' | 'silent';

export interface Tag {
	id: string;
	name: string;
	color: string;
	created_at: string;
}

export interface ConversationUser {
	conversation_id: string;
	user_id: string;
	unread_count: number;
	role: ConversationUserRole;
	joined_at: string;
}

export interface Collaborator {
	conversation_id: string;
	user_id: string;
	username: string;
	unread_count: number;
	role: ConversationUserRole;
	joined_at: string;
}

export interface Conversation {
	id: string;
	title: string | null;
	workspace_id?: string;
	workspace_name?: string;
	workspace_path?: string;
	kind: ConversationKind;
	is_archived: boolean;
	agent_mode: AgentMode;
	unread_count: number;
	last_read_message_id: string | null;
	tags: Tag[];
	created_at: string;
	updated_at: string;
}

export type ContentBlock =
	| { type: 'text'; text: string }
	| { type: 'image'; conversation_attachment_id: string; alt?: string }
	| { type: 'file'; conversation_attachment_id: string }
	| { type: 'audio'; conversation_attachment_id: string }
	| { type: 'video'; conversation_attachment_id: string };

export interface ConversationAttachment {
	id: string;
	kind: 'image' | 'audio' | 'video' | 'document';
	content_type: string;
	filename: string;
	size: number;
	metadata: Record<string, unknown>;
}

export interface Message {
	id: string;
	role: 'user' | 'assistant' | 'system' | 'event';
	content: ContentBlock[];
	reasoning?: string;
	tool_executions?: ToolExecution[];
	token_count: number;
	user_id?: string;
	metadata?: Record<string, unknown>;
	attachments?: Record<string, ConversationAttachment>;
	tags?: Tag[];
	created_at: string;
}

export function getContentText(blocks: ContentBlock[]): string {
	return blocks
		.filter((b): b is Extract<ContentBlock, { type: 'text' }> => b.type === 'text')
		.map((b) => b.text)
		.join('\n');
}

export function getMessageText(msg: Message): string {
	return getContentText(msg.content);
}

export interface ToolExecution {
	id: string;
	tool_call_id: string;
	tool_name: string;
	input: unknown;
	source: 'builtin' | 'plugin' | 'mcp';
	status: 'pending' | 'running' | 'completed' | 'failed' | 'cancelled';
	output?: string;
	error?: string;
	started_at?: string;
	completed_at?: string;
	/** Client-side timestamp when execution first appeared (ms since epoch). */
	_startedAt?: number;
	/** Client-side computed duration in ms (set when execution completes). */
	_durationMs?: number;
}

export interface ConfirmRequest {
	confirm_id: string;
	command: string;
	risk_level: 'safe' | 'moderate' | 'dangerous';
	affects: string[];
	reason: string;
}

// WebSocket message types

/** Client-to-server messages — all include conversation_id */
export type ClientWsMessage =
	| { type: 'chat.subscribe'; conversation_id: string }
	| { type: 'chat.message'; conversation_id: string; content: ContentBlock[] }
	| { type: 'chat.cancel'; conversation_id: string }
	| {
			type: 'chat.confirm_response';
			conversation_id: string;
			confirm_id: string;
			approved: boolean;
	  }
	| {
			type: 'chat.set_permission_mode';
			conversation_id: string;
			mode: PermissionMode;
	  };

/** Server-to-client messages — routed by conversation_id */
export type ServerWsMessage =
	| { type: 'chat.delta'; conversation_id: string; content: string }
	| { type: 'chat.thinking'; conversation_id: string; content: string }
	| { type: 'chat.agent_typing'; conversation_id: string }
	| {
			type: 'chat.tool_execution_update';
			conversation_id: string;
			id: string;
			message_id: string;
			tool_call_id: string;
			tool_name: string;
			status: 'pending' | 'running' | 'completed' | 'failed' | 'cancelled';
			output?: string;
			error?: string;
			input?: string;
	  }
	| { type: 'chat.done'; conversation_id: string; message_id: string; content?: string }
	| {
			type: 'chat.new_message';
			conversation_id: string;
			message_id: string;
			role: string;
			content: ContentBlock[];
			source: MessageSource;
			user_id?: string;
			username?: string;
	  }
	| { type: 'chat.title'; conversation_id: string; title: string }
	| { type: 'chat.error'; conversation_id: string; error: string }
	| {
			type: 'chat.confirm';
			conversation_id: string;
			confirm_id: string;
			command: string;
			risk_level: string;
			affects: string[];
			reason: string;
	  }
	| {
			type: 'chat.message_updated';
			conversation_id: string;
			message_id: string;
			content: string;
	  }
	| { type: 'chat.unread'; conversation_id: string; unread_count: number }
	| {
			type: 'chat.collaborator_added';
			conversation_id: string;
			user: { id: string; username: string };
			role: string;
	  }
	| { type: 'chat.collaborator_removed'; conversation_id: string; user_id: string }
	| { type: 'chat.role_changed'; conversation_id: string; user_id: string; role: string }
	| { type: 'pong' };

export type PermissionMode = 'interactive' | 'policy_based' | 'autonomous';

export interface Workspace {
	id: string;
	name: string;
	path: string;
	created_at: string;
}

export type SandboxNetMode = 'none' | 'allowed_domains' | 'full';

export interface ConversationSettings {
	permission_mode: PermissionMode;
	agent_mode: AgentMode;
	sandbox_profile: string;
	sandbox_net_mode?: SandboxNetMode;
	sandbox_allowed_domains?: string[];
	sandbox_max_execution_seconds?: number;
	sandbox_allow_spawn?: boolean;
	auto_snapshot: boolean;
	max_snapshots?: number;
	disabled_tools: string[];
	disabled_plugins: string[];
}

export interface ToolInfo {
	name: string;
	description: string;
	source: 'builtin' | 'plugin';
	plugin_id?: string;
	plugin_name?: string;
}

/** @deprecated Use ConversationSettings instead */
export interface WorkspaceSettings {
	permission_mode: PermissionMode;
	auto_snapshot: boolean;
}

export interface Job {
	id: string;
	name: string;
	schedule: string;
	status: 'active' | 'paused' | 'cancelled' | 'running';
	next_run_at: string;
	last_run_at: string | null;
}

// API response envelope types

export interface ApiData<T> {
	data: T;
}

export interface ApiError {
	error: {
		code: string;
		message: string;
	};
}

export interface SkillInfo {
	name: string;
	description: string;
}

export type EvolutionType = 'plugin' | 'skill' | 'instruction' | 'automation';
export type EvolutionStatus =
	| 'proposed'
	| 'approved'
	| 'executing'
	| 'active'
	| 'failed'
	| 'rejected'
	| 'reverted';
export type AutonomyLevel = 'auto' | 'approval_required' | 'disabled';

export interface EvolutionEvent {
	id: string;
	evolution_type: EvolutionType;
	user_id: string | null;
	title: string;
	description: string;
	confidence: number;
	source_count: number;
	status: EvolutionStatus;
	payload: Record<string, unknown>;
	result: Record<string, unknown> | null;
	status_history: Array<{ status: string; at: string; by?: string | null }>;
	decided_by: string | null;
	reverted_at: string | null;
	created_at: string;
	updated_at: string;
}

export interface EvolutionConfig {
	interval: string;
	plugin_autonomy: AutonomyLevel;
	skill_autonomy: AutonomyLevel;
	instruction_autonomy: AutonomyLevel;
	automation_autonomy: AutonomyLevel;
}
