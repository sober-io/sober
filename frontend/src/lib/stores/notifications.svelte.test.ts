import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Mock $app/navigation and $app/paths before importing the module under test
vi.mock('$app/navigation', () => ({ goto: vi.fn() }));
vi.mock('$app/paths', () => ({
	resolve: (route: string, params?: Record<string, string>) =>
		route.replace(/\[(\w+)\]/g, (_, key) => params?.[key] ?? '')
}));

import { notifications } from './notifications.svelte';
import { goto } from '$app/navigation';

let mockPermission = 'default';
const mockNotificationInstance = { close: vi.fn(), onclick: null as (() => void) | null };
const mockRequestPermission = vi.fn();
const MockNotification = Object.assign(
	vi.fn(() => mockNotificationInstance),
	{
		requestPermission: mockRequestPermission
	}
);
Object.defineProperty(MockNotification, 'permission', { get: () => mockPermission });

beforeEach(() => {
	mockPermission = 'default';
	vi.stubGlobal('Notification', MockNotification);
	vi.stubGlobal('document', { ...document, hidden: false });
	MockNotification.mockClear();
	mockRequestPermission.mockReset();
	mockNotificationInstance.onclick = null;
	vi.mocked(goto).mockReset();
});

afterEach(() => {
	vi.unstubAllGlobals();
});

describe('notifications.requestPermission', () => {
	it('calls Notification.requestPermission when permission is default', async () => {
		mockRequestPermission.mockResolvedValue('granted');
		await notifications.requestPermission();
		expect(mockRequestPermission).toHaveBeenCalledOnce();
	});

	it('does not call requestPermission when already granted', async () => {
		mockPermission = 'granted';
		await notifications.requestPermission();
		expect(mockRequestPermission).not.toHaveBeenCalled();
	});

	it('does not call requestPermission when denied', async () => {
		mockPermission = 'denied';
		await notifications.requestPermission();
		expect(mockRequestPermission).not.toHaveBeenCalled();
	});
});

describe('notifications.notify', () => {
	it('creates a Notification when permission is granted and tab is hidden', () => {
		mockPermission = 'granted';
		vi.stubGlobal('document', { hidden: true });

		notifications.notify({
			conversationId: 'conv-1',
			title: 'Alice',
			body: 'Hello there'
		});

		expect(MockNotification).toHaveBeenCalledWith('Alice', {
			body: 'Hello there',
			tag: 'conv-1'
		});
	});

	it('does not create a Notification when tab is focused', () => {
		mockPermission = 'granted';
		vi.stubGlobal('document', { hidden: false });

		notifications.notify({
			conversationId: 'conv-1',
			title: 'Alice',
			body: 'Hello there'
		});

		expect(MockNotification).not.toHaveBeenCalled();
	});

	it('does not create a Notification when permission is not granted', () => {
		mockPermission = 'default';
		vi.stubGlobal('document', { hidden: true });

		notifications.notify({
			conversationId: 'conv-1',
			title: 'Alice',
			body: 'Hello there'
		});

		expect(MockNotification).not.toHaveBeenCalled();
	});

	it('navigates to conversation on notification click', () => {
		mockPermission = 'granted';
		vi.stubGlobal('document', { hidden: true });
		vi.stubGlobal('window', { ...window, focus: vi.fn() });

		notifications.notify({
			conversationId: 'conv-1',
			title: 'Alice',
			body: 'Hello there'
		});

		// Simulate click
		mockNotificationInstance.onclick!();
		expect(window.focus).toHaveBeenCalled();
		expect(goto).toHaveBeenCalledWith('/(app)/chat/conv-1');
	});
});
