import { api, ApiError } from '$lib/utils/api';
import { auth } from '$lib/stores/auth.svelte';
import type { User } from '$lib/types';

export const ssr = false;

export async function load() {
	try {
		const user = await api<User>('/auth/me');
		auth.setUser(user);
	} catch (e) {
		if (e instanceof ApiError && e.status === 401) {
			auth.setUser(null);
		} else {
			auth.setUser(null);
		}
	} finally {
		auth.setLoading(false);
	}
}
