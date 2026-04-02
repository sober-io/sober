import { api } from '$lib/utils/api';
import type {
	GatewayPlatform,
	GatewayChannelMapping,
	GatewayUserMapping,
	ExternalChannel,
	CreatePlatformInput,
	UpdatePlatformInput,
	CreateMappingInput,
	CreateUserMappingInput
} from '$lib/types/gateway';

const BASE = '/admin/gateway';

export const gatewayService = {
	listPlatforms: () => api<GatewayPlatform[]>(`${BASE}/platforms`),

	getPlatform: (id: string) => api<GatewayPlatform>(`${BASE}/platforms/${id}`),

	createPlatform: (input: CreatePlatformInput) =>
		api<GatewayPlatform>(`${BASE}/platforms`, {
			method: 'POST',
			body: JSON.stringify(input)
		}),

	updatePlatform: (id: string, input: UpdatePlatformInput) =>
		api<GatewayPlatform>(`${BASE}/platforms/${id}`, {
			method: 'PATCH',
			body: JSON.stringify(input)
		}),

	deletePlatform: (id: string) =>
		api<{ deleted: boolean }>(`${BASE}/platforms/${id}`, { method: 'DELETE' }),

	listChannels: (platformId: string) =>
		api<ExternalChannel[]>(`${BASE}/platforms/${platformId}/channels`),

	listMappings: (platformId: string) =>
		api<GatewayChannelMapping[]>(`${BASE}/platforms/${platformId}/mappings`),

	createMapping: (platformId: string, input: CreateMappingInput) =>
		api<GatewayChannelMapping>(`${BASE}/platforms/${platformId}/mappings`, {
			method: 'POST',
			body: JSON.stringify(input)
		}),

	deleteMapping: (id: string) =>
		api<{ deleted: boolean }>(`${BASE}/mappings/${id}`, { method: 'DELETE' }),

	listUserMappings: (platformId: string) =>
		api<GatewayUserMapping[]>(`${BASE}/platforms/${platformId}/users`),

	createUserMapping: (platformId: string, input: CreateUserMappingInput) =>
		api<GatewayUserMapping>(`${BASE}/platforms/${platformId}/users`, {
			method: 'POST',
			body: JSON.stringify(input)
		}),

	deleteUserMapping: (id: string) =>
		api<{ deleted: boolean }>(`${BASE}/user-mappings/${id}`, { method: 'DELETE' })
};
