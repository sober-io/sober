import { redirect } from '@sveltejs/kit';
import { auth } from '$lib/stores/auth.svelte';

export async function load({ parent }: { parent: () => Promise<void> }) {
	await parent();
	if (auth.isAuthenticated) {
		redirect(302, '/');
	}
}
