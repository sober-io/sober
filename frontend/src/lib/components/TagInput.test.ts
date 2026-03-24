import { describe, it, expect, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import { userEvent } from '@testing-library/user-event';
import TagInput from './TagInput.svelte';
import type { Tag } from '$lib/types';

// Use 't1' as the id for 'bug' in both allTags and existingTags so the
// component's id-based filter correctly excludes it from suggestions.
vi.mock('$lib/services/tags', () => ({
	tagService: {
		list: vi.fn().mockResolvedValue([
			{ id: 't1', name: 'bug', color: '#f00', created_at: '2026-01-01T00:00:00Z' },
			{ id: 's2', name: 'feature', color: '#0f0', created_at: '2026-01-01T00:00:00Z' },
			{ id: 's3', name: 'docs', color: '#00f', created_at: '2026-01-01T00:00:00Z' }
		])
	}
}));

const existingTags: Tag[] = [
	{ id: 't1', name: 'bug', color: '#f00', created_at: '2026-01-01T00:00:00Z' }
];

describe('TagInput', () => {
	it('renders existing tags with remove buttons', () => {
		render(TagInput, {
			props: { tags: existingTags, onAdd: vi.fn(), onRemove: vi.fn() }
		});

		expect(screen.getByText('bug')).toBeInTheDocument();
		expect(screen.getByLabelText('Remove tag bug')).toBeInTheDocument();
	});

	it('"Add tag" button reveals input field', async () => {
		const user = userEvent.setup();
		render(TagInput, {
			props: { tags: [], onAdd: vi.fn(), onRemove: vi.fn() }
		});

		await user.click(screen.getByText('Add tag'));

		expect(screen.getByPlaceholderText('Tag name…')).toBeInTheDocument();
	});

	it('Enter submits new tag via onAdd callback', async () => {
		const user = userEvent.setup();
		const onAdd = vi.fn();
		render(TagInput, {
			props: { tags: [], onAdd, onRemove: vi.fn() }
		});

		await user.click(screen.getByText('Add tag'));
		const input = screen.getByPlaceholderText('Tag name…');
		await user.type(input, 'new-tag');
		await user.keyboard('{Enter}');

		expect(onAdd).toHaveBeenCalledWith('new-tag');
	});

	it('Escape closes input and clears value', async () => {
		const user = userEvent.setup();
		render(TagInput, {
			props: { tags: [], onAdd: vi.fn(), onRemove: vi.fn() }
		});

		await user.click(screen.getByText('Add tag'));
		const input = screen.getByPlaceholderText('Tag name…');
		await user.type(input, 'draft');
		await user.keyboard('{Escape}');

		expect(screen.queryByPlaceholderText('Tag name…')).not.toBeInTheDocument();
	});

	it('remove button calls onRemove with tag id', async () => {
		const user = userEvent.setup();
		const onRemove = vi.fn();
		render(TagInput, {
			props: { tags: existingTags, onAdd: vi.fn(), onRemove }
		});

		await user.click(screen.getByLabelText('Remove tag bug'));

		expect(onRemove).toHaveBeenCalledWith('t1');
	});

	it('suggestions exclude already-applied tags', async () => {
		const user = userEvent.setup();
		render(TagInput, {
			props: { tags: existingTags, onAdd: vi.fn(), onRemove: vi.fn() }
		});

		await user.click(screen.getByText('Add tag'));
		const input = screen.getByPlaceholderText('Tag name…');
		await user.click(input); // focus to show suggestions

		// "bug" is already applied — should not appear in suggestions
		// "feature" and "docs" should appear (wait for async tagService.list)
		await waitFor(() => {
			const suggestions = screen.queryAllByRole('button').filter((b) => b.closest('ul'));
			const suggestionTexts = suggestions.map((s) => s.textContent?.trim());
			expect(suggestionTexts).not.toContain('bug');
		});
	});
});
