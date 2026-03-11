// Domain types mirroring backend API response shapes

export interface User {
	id: string;
	email: string;
	username: string;
	status: string;
}

export interface Conversation {
	id: string;
	title: string;
	created_at: string;
	updated_at: string;
}

export interface ConversationWithMessages extends Conversation {
	messages: Message[];
}

export interface Message {
	id: string;
	role: 'User' | 'Assistant' | 'System';
	content: string;
	tool_calls?: unknown;
	tool_result?: unknown;
	token_count: number;
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
	| { type: 'chat.message'; conversation_id: string; content: string }
	| { type: 'chat.cancel'; conversation_id: string }
	| {
			type: 'chat.confirm_response';
			conversation_id: string;
			confirm_id: string;
			approved: boolean;
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
	  };

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
