import { describe, it, expect } from 'vitest';
import { formatRelativeTime, formatRelativeFuture } from './time';

describe('time utils', () => {
	it('formatRelativeTime returns human-readable past time', () => {
		const twoHoursAgo = new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString();
		const result = formatRelativeTime(twoHoursAgo);

		expect(result).toBe('2 hours ago');
	});

	it('formatRelativeFuture returns human-readable future time', () => {
		const inThreeDays = new Date(Date.now() + 3 * 24 * 60 * 60 * 1000).toISOString();
		const result = formatRelativeFuture(inThreeDays);

		expect(result).toBe('in 3 days');
	});
});
