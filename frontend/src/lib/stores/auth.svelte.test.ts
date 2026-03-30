import { describe, it, expect } from 'vitest';
import { auth } from './auth.svelte';

describe('auth store', () => {
	it('has correct initial state', () => {
		// Reset to initial state
		auth.setUser(null);
		auth.setLoading(true);

		expect(auth.user).toBeNull();
		expect(auth.loading).toBe(true);
		expect(auth.isAuthenticated).toBe(false);
	});

	it('setUser updates user and isAuthenticated becomes true', () => {
		auth.setUser({
			id: '1',
			email: 'a@b.com',
			username: 'alice',
			status: 'active',
			roles: ['user']
		});

		expect(auth.user).toEqual({
			id: '1',
			email: 'a@b.com',
			username: 'alice',
			status: 'active',
			roles: ['user']
		});
		expect(auth.isAuthenticated).toBe(true);
	});

	it('setUser(null) resets isAuthenticated to false', () => {
		auth.setUser({
			id: '1',
			email: 'a@b.com',
			username: 'alice',
			status: 'active',
			roles: ['user']
		});
		expect(auth.isAuthenticated).toBe(true);

		auth.setUser(null);
		expect(auth.isAuthenticated).toBe(false);
	});

	it('setLoading updates loading state', () => {
		auth.setLoading(false);
		expect(auth.loading).toBe(false);

		auth.setLoading(true);
		expect(auth.loading).toBe(true);
	});
});
