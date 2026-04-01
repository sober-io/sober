import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import { userEvent } from '@testing-library/user-event';
import ConversationSettings from './ConversationSettings.svelte';
import type { Conversation, PermissionMode, Tag } from '$lib/types';

vi.mock('$lib/services/conversations', () => ({
	conversationService: {
		listCollaborators: vi.fn().mockResolvedValue([]),
		addCollaborator: vi.fn(),
		removeCollaborator: vi.fn(),
		updateCollaboratorRole: vi.fn(),
		updateAgentMode: vi.fn(),
		convertToGroup: vi.fn(),
		leave: vi.fn()
	}
}));

vi.mock('$lib/services/jobs', () => ({
	jobService: {
		listByConversation: vi.fn().mockResolvedValue([])
	}
}));

vi.mock('$lib/stores/auth.svelte', () => ({
	auth: {
		get user() {
			return { id: 'user-1', email: 'a@b.com', username: 'alice', status: 'active' };
		}
	}
}));

vi.mock('$lib/stores/conversations.svelte', () => ({
	conversations: {
		update: vi.fn()
	}
}));

function makeConversation(overrides: Partial<Conversation> = {}): Conversation {
	return {
		id: 'conv-1',
		title: 'Test Conversation',
		kind: 'direct',
		is_archived: false,
		agent_mode: 'always',
		unread_count: 0,
		last_read_message_id: null,
		tags: [],
		created_at: '2026-01-15T00:00:00Z',
		updated_at: '2026-01-15T00:00:00Z',
		...overrides
	};
}

const defaultProps = {
	open: true,
	conversation: makeConversation(),
	tags: [] as Tag[],
	permissionMode: 'interactive' as PermissionMode,
	onClose: vi.fn(),
	onUpdateTitle: vi.fn(),
	onUpdatePermissionMode: vi.fn(),
	onAddTag: vi.fn(),
	onRemoveTag: vi.fn(),
	onArchive: vi.fn(),
	onClearHistory: vi.fn(),
	onDelete: vi.fn()
};

describe('ConversationSettings', () => {
	beforeEach(() => {
		vi.clearAllMocks();
	});

	it('renders nothing when open is false', () => {
		const { container } = render(ConversationSettings, {
			props: { ...defaultProps, open: false }
		});

		expect(container.querySelector('[role="dialog"]')).not.toBeInTheDocument();
	});

	it('renders panel with title, kind label, and created date when open', async () => {
		render(ConversationSettings, { props: defaultProps });

		await waitFor(() => {
			expect(screen.getByRole('dialog')).toBeInTheDocument();
		});
		expect(screen.getByText('Settings')).toBeInTheDocument();
		expect(screen.getByText('Direct')).toBeInTheDocument();
		expect(screen.getByText(/2026/)).toBeInTheDocument();
	});

	it('calls onUpdateTitle on blur with trimmed value', async () => {
		const user = userEvent.setup();
		const onUpdateTitle = vi.fn();
		render(ConversationSettings, {
			props: { ...defaultProps, onUpdateTitle }
		});

		const input = screen.getByPlaceholderText('Conversation title...');
		await user.clear(input);
		await user.type(input, '  New Title  ');
		await user.tab(); // triggers blur

		expect(onUpdateTitle).toHaveBeenCalledWith('New Title');
	});

	it('archive button text toggles based on is_archived', async () => {
		const { rerender } = render(ConversationSettings, {
			props: defaultProps
		});

		await waitFor(() => {
			expect(screen.getByText('Archive conversation')).toBeInTheDocument();
		});

		rerender({ ...defaultProps, conversation: makeConversation({ is_archived: true }) });

		await waitFor(() => {
			expect(screen.getByText('Unarchive conversation')).toBeInTheDocument();
		});
	});

	it('clear history button opens confirm dialog', async () => {
		const user = userEvent.setup();
		render(ConversationSettings, { props: defaultProps });

		await user.click(screen.getByRole('button', { name: 'Clear message history' }));

		expect(
			screen.getByText(
				'All messages in this conversation will be permanently deleted. This action cannot be undone.'
			)
		).toBeInTheDocument();
	});

	it('delete button hidden for inbox kind', () => {
		render(ConversationSettings, {
			props: { ...defaultProps, conversation: makeConversation({ kind: 'inbox' }) }
		});

		expect(screen.queryByRole('button', { name: 'Delete conversation' })).not.toBeInTheDocument();
	});

	it('delete confirm dialog calls onDelete', async () => {
		const user = userEvent.setup();
		const onDelete = vi.fn();
		render(ConversationSettings, {
			props: { ...defaultProps, onDelete }
		});

		await user.click(screen.getByRole('button', { name: 'Delete conversation' }));
		await user.click(screen.getByRole('button', { name: 'Delete' }));

		expect(onDelete).toHaveBeenCalled();
	});

	it('close button calls onClose', async () => {
		const user = userEvent.setup();
		const onClose = vi.fn();
		render(ConversationSettings, {
			props: { ...defaultProps, onClose }
		});

		await user.click(screen.getByLabelText('Close'));

		expect(onClose).toHaveBeenCalled();
	});
});
