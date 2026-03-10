import type { User } from '$lib/types';

/** Reactive auth state shared across the application. */
export const auth = (() => {
	let user = $state<User | null>(null);
	let loading = $state(true);

	return {
		get user() {
			return user;
		},
		get loading() {
			return loading;
		},
		get isAuthenticated() {
			return user !== null;
		},
		setUser(u: User | null) {
			user = u;
		},
		setLoading(l: boolean) {
			loading = l;
		}
	};
})();
