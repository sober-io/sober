import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import { userEvent } from '@testing-library/user-event';
import MessageTagPopover from './MessageTagPopover.svelte';
import type { Tag } from '$lib/types';

const mockTagService = vi.hoisted(() => ({
	list: vi.fn(),
	addToMessage: vi.fn(),
	removeFromMessage: vi.fn()
}));

vi.mock('$lib/services/tags', () => ({
	tagService: mockTagService
}));

const allTags: Tag[] = [
	{ id: 's1', name: 'bug', color: '#f00', created_at: '2026-01-01T00:00:00Z' },
	{ id: 's2', name: 'feature', color: '#0f0', created_at: '2026-01-01T00:00:00Z' },
	{ id: 's3', name: 'docs', color: '#00f', created_at: '2026-01-01T00:00:00Z' }
];

const appliedTags: Tag[] = [allTags[0]]; // bug is applied

describe('MessageTagPopover', () => {
	beforeEach(() => {
		vi.clearAllMocks();
		mockTagService.list.mockResolvedValue(allTags);
	});

	it('renders existing tags with remove buttons', () => {
		render(MessageTagPopover, {
			props: { messageId: 'msg-1', tags: appliedTags, onTagsChange: vi.fn(), onClose: vi.fn() }
		});

		expect(screen.getByText('bug')).toBeInTheDocument();
		expect(screen.getByLabelText('Remove tag bug')).toBeInTheDocument();
	});

	it('filters suggestions by input text (case-insensitive)', async () => {
		const user = userEvent.setup();
		render(MessageTagPopover, {
			props: { messageId: 'msg-1', tags: [], onTagsChange: vi.fn(), onClose: vi.fn() }
		});

		const input = await screen.findByPlaceholderText('Add tag…');
		await user.type(input, 'FEA');

		await waitFor(() => {
			const suggestions = screen.queryAllByRole('button').filter((b) => b.closest('ul'));
			const texts = suggestions.map((s) => s.textContent?.trim());
			expect(texts).toContain('feature');
			expect(texts).not.toContain('docs');
		});
	});

	it('excludes already-applied tags from suggestions', async () => {
		render(MessageTagPopover, {
			props: { messageId: 'msg-1', tags: appliedTags, onTagsChange: vi.fn(), onClose: vi.fn() }
		});

		// Wait for suggestions to load
		await waitFor(() => {
			const suggestions = screen.queryAllByRole('button').filter((b) => b.closest('ul'));
			const texts = suggestions.map((s) => s.textContent?.trim());
			expect(texts).not.toContain('bug');
		});
	});

	it('clicking suggestion adds tag and fires onTagsChange', async () => {
		const user = userEvent.setup();
		const onTagsChange = vi.fn();
		const newTag = {
			id: 'new-1',
			name: 'feature',
			color: '#0f0',
			created_at: '2026-01-01T00:00:00Z'
		};
		mockTagService.addToMessage.mockResolvedValue(newTag);

		render(MessageTagPopover, {
			props: { messageId: 'msg-1', tags: appliedTags, onTagsChange, onClose: vi.fn() }
		});

		// Wait for suggestions to appear
		const featureBtn = await screen.findByRole('button', { name: /feature/i });
		await user.click(featureBtn);

		await waitFor(() => {
			expect(mockTagService.addToMessage).toHaveBeenCalledWith('msg-1', 'feature');
			expect(onTagsChange).toHaveBeenCalledWith([...appliedTags, newTag]);
		});
	});

	it('Escape key calls onClose', async () => {
		const user = userEvent.setup();
		const onClose = vi.fn();
		render(MessageTagPopover, {
			props: { messageId: 'msg-1', tags: [], onTagsChange: vi.fn(), onClose }
		});

		const input = await screen.findByPlaceholderText('Add tag…');
		await user.type(input, '{Escape}');

		expect(onClose).toHaveBeenCalled();
	});
});
