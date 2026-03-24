<script lang="ts">
	import { resolve } from '$app/paths';
	import { ApiError } from '$lib/utils/api';
	import { authService } from '$lib/services/auth';
	import { systemService } from '$lib/services/system';
	import type { User } from '$lib/types';

	let email = $state('');
	let username = $state('');
	let password = $state('');
	let error = $state<string | null>(null);
	let submitting = $state(false);
	let registeredUser = $state<User | null>(null);
	let initialized = $state<boolean | null>(null);

	$effect(() => {
		systemService.status().then((s) => {
			initialized = s.initialized;
		});
	});

	let isFirstUser = $derived(initialized === false);

	const handleSubmit = async (e: SubmitEvent) => {
		e.preventDefault();
		error = null;
		submitting = true;

		try {
			registeredUser = await authService.register(email, username, password);
		} catch (err) {
			if (err instanceof ApiError) {
				error = err.message;
			} else {
				error = 'An unexpected error occurred';
			}
		} finally {
			submitting = false;
		}
	};
</script>

<div
	class="rounded-lg border border-zinc-200 bg-white p-8 shadow-sm dark:border-zinc-800 dark:bg-zinc-900"
>
	{#if registeredUser}
		<div class="text-center">
			{#if registeredUser.status === 'Active'}
				<h1 class="mb-4 text-2xl font-semibold text-zinc-900 dark:text-zinc-100">
					Account created!
				</h1>
				<p class="mb-6 text-sm text-zinc-600 dark:text-zinc-400">You can now sign in.</p>
				<a href={resolve('/login')} class="text-sm text-zinc-900 underline dark:text-zinc-100">
					Sign in
				</a>
			{:else}
				<h1 class="mb-4 text-2xl font-semibold text-zinc-900 dark:text-zinc-100">
					Registration submitted
				</h1>
				<p class="mb-6 text-sm text-zinc-600 dark:text-zinc-400">
					Your account is pending approval. You'll be able to sign in once an administrator approves
					your account.
				</p>
				<a href={resolve('/login')} class="text-sm text-zinc-900 underline dark:text-zinc-100">
					Back to sign in
				</a>
			{/if}
		</div>
	{:else}
		{#if isFirstUser}
			<div
				class="mb-6 rounded-md border border-emerald-200 bg-emerald-50 p-4 text-sm text-emerald-800 dark:border-emerald-800 dark:bg-emerald-950 dark:text-emerald-200"
			>
				Welcome to Sober! Create your admin account to get started.
			</div>
		{/if}

		<h1 class="mb-6 text-2xl font-semibold text-zinc-900 dark:text-zinc-100">
			{isFirstUser ? 'Create admin account' : 'Create account'}
		</h1>

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
				<label
					for="username"
					class="mb-1 block text-sm font-medium text-zinc-700 dark:text-zinc-300"
				>
					Username
				</label>
				<input
					id="username"
					type="text"
					bind:value={username}
					required
					autocomplete="username"
					class="w-full rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400 dark:focus:ring-zinc-400"
				/>
			</div>

			<div>
				<label
					for="password"
					class="mb-1 block text-sm font-medium text-zinc-700 dark:text-zinc-300"
				>
					Password
				</label>
				<input
					id="password"
					type="password"
					bind:value={password}
					required
					autocomplete="new-password"
					class="w-full rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 placeholder:text-zinc-400 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-zinc-400 dark:focus:ring-zinc-400"
				/>
			</div>

			<button
				type="submit"
				disabled={submitting}
				class="w-full rounded-md bg-zinc-900 px-4 py-2 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
			>
				{#if submitting}
					Creating account...
				{:else if isFirstUser}
					Create Admin Account
				{:else}
					Create account
				{/if}
			</button>
		</form>

		<p class="mt-4 text-center text-sm text-zinc-500 dark:text-zinc-400">
			Already have an account?
			<a href={resolve('/login')} class="text-zinc-900 underline dark:text-zinc-100">Sign in</a>
		</p>
	{/if}
</div>
