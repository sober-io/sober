import { redirect } from '@sveltejs/kit';
import { isAdmin } from '$lib/guards';

export async function load({ parent }: { parent: () => Promise<void> }) {
	await parent();
	if (!isAdmin()) {
		redirect(302, '/settings');
	}
}
