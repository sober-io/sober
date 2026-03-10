import type { ApiData } from '$lib/types';

export class ApiError extends Error {
	status: number;
	code: string;

	constructor(status: number, body: { error?: { code?: string; message?: string } }) {
		super(body?.error?.message ?? `API error ${status}`);
		this.status = status;
		this.code = body?.error?.code ?? 'unknown';
	}
}

/** Typed API client that unwraps the `{ data: T }` response envelope. */
export async function api<T>(path: string, options?: RequestInit): Promise<T> {
	const res = await fetch(`/api/v1${path}`, {
		headers: { 'Content-Type': 'application/json' },
		credentials: 'include',
		...options
	});

	if (!res.ok) {
		const body = await res.json().catch(() => ({}));
		throw new ApiError(res.status, body);
	}

	const json: ApiData<T> = await res.json();
	return json.data;
}
