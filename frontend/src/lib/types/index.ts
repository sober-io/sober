// Domain types mirroring backend API response shapes

export interface User {
	id: string;
	email: string;
	username: string;
	status: string;
}

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
	permission_mode: PermissionMode;
	agent_mode: AgentMode;
	unread_count: number;
	tags: Tag[];
	created_at: string;
	updated_at: string;
}

export interface Message {
	id: string;
	role: 'user' | 'assistant' | 'system' | 'event';
	content: string;
	reasoning?: string;
	tool_executions?: ToolExecution[];
	token_count: number;
	user_id?: string;
	metadata?: Record<string, unknown>;
	tags?: Tag[];
	created_at: string;
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
	| { type: 'chat.message'; conversation_id: string; content: string }
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
	| { type: 'chat.done'; conversation_id: string; message_id: string }
	| {
			type: 'chat.new_message';
			conversation_id: string;
			message_id: string;
			role: string;
			content: string;
			source: string;
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
