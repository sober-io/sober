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
		}
		// Non-auth errors (429, 500, network) — keep existing auth state.
		// The user will see errors on actual data fetches instead of being
		// silently logged out.
	} finally {
		auth.setLoading(false);
	}
};
