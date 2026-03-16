// Domain types mirroring backend API response shapes

export interface User {
	id: string;
	email: string;
	username: string;
	status: string;
}

export type ConversationKind = 'direct' | 'group' | 'inbox';
export type ConversationUserRole = 'owner' | 'member';

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

export interface Conversation {
	id: string;
	title: string | null;
	workspace_id?: string;
	kind: ConversationKind;
	is_archived: boolean;
	permission_mode: PermissionMode;
	unread_count: number;
	tags: Tag[];
	created_at: string;
	updated_at: string;
}

export interface Message {
	id: string;
	role: 'User' | 'Assistant' | 'System' | 'Tool';
	content: string;
	tool_calls?: unknown;
	tool_result?: unknown;
	token_count: number;
	user_id?: string;
	created_at: string;
}

export interface ToolCall {
	id: string;
	name: string;
	input: unknown;
	output?: string;
}

export interface McpServer {
	id: string;
	name: string;
	command: string;
	args: unknown[];
	env: Record<string, string>;
	enabled: boolean;
	created_at: string;
	updated_at: string;
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
	| { type: 'chat.tool_use'; conversation_id: string; tool_call: { name: string; input: unknown } }
	| {
			type: 'chat.tool_result';
			conversation_id: string;
			tool_call_id: string;
			output: string;
	  }
	| { type: 'chat.done'; conversation_id: string; message_id: string }
	| {
			type: 'chat.new_message';
			conversation_id: string;
			message_id: string;
			role: string;
			content: string;
			source: string;
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
	| { type: 'pong' };

export type PermissionMode = 'interactive' | 'policy_based' | 'autonomous';

export interface WorkspaceSettings {
	permission_mode: PermissionMode;
	auto_snapshot: boolean;
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
