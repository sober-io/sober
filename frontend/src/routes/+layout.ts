import { ApiError } from '$lib/utils/api';
import { authService } from '$lib/services/auth';
import { auth } from '$lib/stores/auth.svelte';

export const ssr = false;

export const load = async () => {
	try {
		const user = await authService.me();
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
};
