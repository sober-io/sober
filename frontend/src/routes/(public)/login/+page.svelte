<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { api, ApiError } from '$lib/utils/api';
	import { auth } from '$lib/stores/auth.svelte';
	import type { User } from '$lib/types';

	let email = $state('');
	let password = $state('');
	let error = $state<string | null>(null);
	let submitting = $state(false);

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		error = null;
		submitting = true;

		try {
			const result = await api<{ token: string; user: User }>('/auth/login', {
				method: 'POST',
				body: JSON.stringify({ email, password })
			});
			auth.setUser(result.user);
			goto(resolve('/'));
		} catch (err) {
			if (err instanceof ApiError) {
				error = err.message;
			} else {
				error = 'An unexpected error occurred';
			}
		} finally {
			submitting = false;
		}
	}
</script>

<div
	class="rounded-lg border border-zinc-200 bg-white p-8 shadow-sm dark:border-zinc-800 dark:bg-zinc-900"
>
	<h1 class="mb-6 text-2xl font-semibold text-zinc-900 dark:text-zinc-100">Sign in</h1>

	{#if error}
		<div
			class="mb-4 rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-950 dark:text-red-300"
		>
			{error}
		</div>
	{/if}

	<form onsubmit={handleSubmit} class="space-y-4">
		<div>
			<label for="email" class="mb-1 block text-sm font-medium text-zinc-700 dark:text-zinc-300">
				Email
			</label>
			<input
				id="email"
				type="email"
				bind:value={email}
				required
				autocomplete="email"
				class="w-full rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400 dark:focus:ring-zinc-400"
			/>
		</div>

		<div>
			<label for="password" class="mb-1 block text-sm font-medium text-zinc-700 dark:text-zinc-300">
				Password
			</label>
			<input
				id="password"
				type="password"
				bind:value={password}
				required
				autocomplete="current-password"
				class="w-full rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400 dark:focus:ring-zinc-400"
			/>
		</div>

		<button
			type="submit"
			disabled={submitting}
			class="w-full rounded-md bg-zinc-900 px-4 py-2 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
		>
			{submitting ? 'Signing in...' : 'Sign in'}
		</button>
	</form>

	<p class="mt-4 text-center text-sm text-zinc-500 dark:text-zinc-400">
		Don't have an account?
		<a href={resolve('/register')} class="text-zinc-900 underline dark:text-zinc-100">Register</a>
	</p>
</div>
