import { api } from '$lib/utils/api';
import type { User } from '$lib/types';

export const authService = {
	login: (email: string, password: string) =>
		api<{ token: string; user: User }>('/auth/login', {
			method: 'POST',
			body: JSON.stringify({ email, password })
		}),

	register: (email: string, username: string, password: string) =>
		api('/auth/register', {
			method: 'POST',
			body: JSON.stringify({ email, username, password })
		}),

	logout: () => api('/auth/logout', { method: 'POST' }),

	me: () => api<User>('/auth/me')
};
