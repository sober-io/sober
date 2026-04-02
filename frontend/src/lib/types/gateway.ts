// Gateway domain types mirroring the backend admin gateway API.

export type PlatformType = 'discord' | 'telegram' | 'matrix' | 'whatsapp';

export interface GatewayPlatform {
	id: string;
	platform_type: PlatformType;
	display_name: string;
	is_enabled: boolean;
	created_at: string;
	updated_at: string;
}

export interface GatewayChannelMapping {
	id: string;
	platform_id: string;
	external_channel_id: string;
	external_channel_name: string;
	conversation_id: string;
	is_thread: boolean;
	parent_mapping_id: string | null;
	created_at: string;
}

export interface GatewayUserMapping {
	id: string;
	platform_id: string;
	external_user_id: string;
	external_username: string;
	user_id: string;
	created_at: string;
}

export interface ExternalChannel {
	id: string;
	name: string;
	kind: string;
}

export interface CreatePlatformInput {
	platform_type: string;
	display_name: string;
}

export interface UpdatePlatformInput {
	display_name?: string;
	is_enabled?: boolean;
}

export interface CreateMappingInput {
	external_channel_id: string;
	external_channel_name: string;
	conversation_id: string;
}

export interface CreateUserMappingInput {
	external_user_id: string;
	external_username: string;
	user_id: string;
}
