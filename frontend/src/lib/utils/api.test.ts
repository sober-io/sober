import { describe, it, expect, vi, beforeEach } from 'vitest';
import { api, ApiError } from './api';

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

beforeEach(() => {
	mockFetch.mockReset();
});

function jsonResponse(data: unknown, status = 200) {
	return {
		ok: status >= 200 && status < 300,
		status,
		json: () => Promise.resolve(data)
	};
}

describe('api', () => {
	it('unwraps { data: T } envelope on success', async () => {
		mockFetch.mockResolvedValue(jsonResponse({ data: { id: '1', name: 'test' } }));

		const result = await api<{ id: string; name: string }>('/users/1');

		expect(result).toEqual({ id: '1', name: 'test' });
	});

	it('prepends /api/v1 and sets default headers and credentials', async () => {
		mockFetch.mockResolvedValue(jsonResponse({ data: null }));

		await api('/users');

		expect(mockFetch).toHaveBeenCalledWith('/api/v1/users', {
			headers: { 'Content-Type': 'application/json' },
			credentials: 'include'
		});
	});

	it('spreads custom RequestInit options', async () => {
		mockFetch.mockResolvedValue(jsonResponse({ data: null }));

		await api('/auth/login', {
			method: 'POST',
			body: JSON.stringify({ email: 'a@b.com' })
		});

		expect(mockFetch).toHaveBeenCalledWith('/api/v1/auth/login', {
			headers: { 'Content-Type': 'application/json' },
			credentials: 'include',
			method: 'POST',
			body: JSON.stringify({ email: 'a@b.com' })
		});
	});

	it('throws ApiError with status, code, and message on non-ok response', async () => {
		mockFetch.mockResolvedValue({
			ok: false,
			status: 422,
			json: () => Promise.resolve({ error: { code: 'validation', message: 'Invalid email' } })
		});

		const err = await api('/users').catch((e: ApiError) => e);

		expect(err).toBeInstanceOf(ApiError);
		expect(err.status).toBe(422);
		expect(err.code).toBe('validation');
		expect(err.message).toBe('Invalid email');
	});

	it('falls back to defaults on malformed error body', async () => {
		mockFetch.mockResolvedValue({
			ok: false,
			status: 500,
			json: () => Promise.resolve({ unexpected: true })
		});

		const err = await api('/fail').catch((e: ApiError) => e);

		expect(err).toBeInstanceOf(ApiError);
		expect(err.status).toBe(500);
		expect(err.code).toBe('unknown');
		expect(err.message).toBe('API error 500');
	});

	it('handles non-JSON error body gracefully', async () => {
		mockFetch.mockResolvedValue({
			ok: false,
			status: 502,
			json: () => Promise.reject(new Error('not json'))
		});

		const err = await api('/fail').catch((e: ApiError) => e);

		expect(err).toBeInstanceOf(ApiError);
		expect(err.status).toBe(502);
		expect(err.code).toBe('unknown');
		expect(err.message).toBe('API error 502');
	});
});
