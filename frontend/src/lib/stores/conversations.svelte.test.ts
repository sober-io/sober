import { describe, it, expect, beforeEach } from 'vitest';
import { conversations } from './conversations.svelte';
import type { Conversation, Tag } from '$lib/types';

function makeConversation(overrides: Partial<Conversation> = {}): Conversation {
	return {
		id: crypto.randomUUID(),
		title: 'Test',
		kind: 'direct',
		is_archived: false,
		permission_mode: 'interactive',
		agent_mode: 'always',
		unread_count: 0,
		tags: [],
		created_at: '2026-01-01T00:00:00Z',
		updated_at: '2026-01-01T00:00:00Z',
		...overrides
	};
}

describe('conversations store', () => {
	beforeEach(() => {
		conversations.set([]);
	});

	it('set replaces the items list', () => {
		const list = [makeConversation({ id: 'a' }), makeConversation({ id: 'b' })];
		conversations.set(list);

		expect(conversations.items).toHaveLength(2);
		expect(conversations.items[0].id).toBe('a');
	});

	it('prepend adds conversation to front', () => {
		conversations.set([makeConversation({ id: 'existing' })]);
		conversations.prepend(makeConversation({ id: 'new' }));

		expect(conversations.items[0].id).toBe('new');
		expect(conversations.items).toHaveLength(2);
	});

	it('updateTitle updates matching conversation', () => {
		conversations.set([makeConversation({ id: 'a', title: 'Old' })]);
		conversations.updateTitle('a', 'New Title');

		expect(conversations.items[0].title).toBe('New Title');
	});

	it('remove filters out conversation by id', () => {
		conversations.set([makeConversation({ id: 'a' }), makeConversation({ id: 'b' })]);
		conversations.remove('a');

		expect(conversations.items).toHaveLength(1);
		expect(conversations.items[0].id).toBe('b');
	});

	it('updateUnread sets count and sorts unread first, then by updated_at', () => {
		const old = makeConversation({
			id: 'old',
			updated_at: '2026-01-01T00:00:00Z',
			unread_count: 0
		});
		const recent = makeConversation({
			id: 'recent',
			updated_at: '2026-03-01T00:00:00Z',
			unread_count: 0
		});
		conversations.set([old, recent]);

		// Mark the older one as unread — it should jump to front
		conversations.updateUnread('old', 3);

		expect(conversations.items[0].id).toBe('old');
		expect(conversations.items[0].unread_count).toBe(3);
		expect(conversations.items[1].id).toBe('recent');
	});

	it('updateUnread sort stability: two unread items sorted by updated_at', () => {
		const older = makeConversation({
			id: 'older',
			updated_at: '2026-01-01T00:00:00Z',
			unread_count: 1
		});
		const newer = makeConversation({
			id: 'newer',
			updated_at: '2026-03-01T00:00:00Z',
			unread_count: 1
		});
		const read = makeConversation({
			id: 'read',
			updated_at: '2026-02-01T00:00:00Z',
			unread_count: 0
		});
		conversations.set([older, newer, read]);

		// Trigger sort by updating any unread
		conversations.updateUnread('older', 2);

		expect(conversations.items[0].id).toBe('newer');
		expect(conversations.items[1].id).toBe('older');
		expect(conversations.items[2].id).toBe('read');
	});

	it('markRead sets unread_count to 0', () => {
		conversations.set([makeConversation({ id: 'a', unread_count: 5 })]);
		conversations.markRead('a');

		expect(conversations.items[0].unread_count).toBe(0);
	});

	it('archive and unarchive toggle is_archived', () => {
		conversations.set([makeConversation({ id: 'a', is_archived: false })]);

		conversations.archive('a');
		expect(conversations.items[0].is_archived).toBe(true);

		conversations.unarchive('a');
		expect(conversations.items[0].is_archived).toBe(false);
	});

	it('updateTags replaces tags on matching conversation', () => {
		conversations.set([makeConversation({ id: 'a', tags: [] })]);

		const newTags: Tag[] = [
			{ id: 't1', name: 'bug', color: '#f00', created_at: '2026-01-01T00:00:00Z' }
		];
		conversations.updateTags('a', newTags);

		expect(conversations.items[0].tags).toEqual(newTags);
	});

	it('update merges partial fields into matching conversation', () => {
		conversations.set([makeConversation({ id: 'a', title: 'Old', kind: 'direct' })]);
		conversations.update('a', { title: 'Updated', kind: 'group' });

		expect(conversations.items[0].title).toBe('Updated');
		expect(conversations.items[0].kind).toBe('group');
	});
});
