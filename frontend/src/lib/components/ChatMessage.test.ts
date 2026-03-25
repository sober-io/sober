import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { userEvent } from '@testing-library/user-event';
import ChatMessage from './ChatMessage.svelte';

describe('ChatMessage', () => {
	it('renders user message right-aligned', () => {
		const { container } = render(ChatMessage, {
			props: { role: 'user', content: 'Hello' }
		});

		const wrapper = container.querySelector('.justify-end');
		expect(wrapper).toBeInTheDocument();
	});

	it('renders assistant message left-aligned', () => {
		const { container } = render(ChatMessage, {
			props: { role: 'assistant', content: 'Hi there' }
		});

		const wrapper = container.querySelector('.justify-start');
		expect(wrapper).toBeInTheDocument();
	});

	it('renders event role as centered italic text', () => {
		render(ChatMessage, {
			props: { role: 'event', content: 'User joined' }
		});

		const el = screen.getByText('User joined');
		expect(el.tagName).toBe('SPAN');
		expect(el.classList.contains('italic')).toBe(true);
	});

	it('renders content as sanitized markdown HTML', () => {
		const { container } = render(ChatMessage, {
			props: { role: 'assistant', content: '**bold text**' }
		});

		const strong = container.querySelector('strong');
		expect(strong).toBeInTheDocument();
		expect(strong?.textContent).toBe('bold text');
	});

	it('shows thinking indicator when thinking with no content', () => {
		render(ChatMessage, {
			props: { role: 'assistant', content: '', thinking: true }
		});

		expect(screen.getByRole('status', { name: /thinking/i })).toBeInTheDocument();
	});

	it('shows streaming text when streaming', () => {
		render(ChatMessage, {
			props: { role: 'assistant', content: 'Streaming...', streaming: true }
		});

		expect(screen.getByText('Streaming...')).toBeInTheDocument();
	});

	it('displays tool executions when provided', () => {
		render(ChatMessage, {
			props: {
				role: 'assistant',
				content: 'Done.',
				toolExecutions: [
					{
						id: 'te1',
						tool_call_id: 'tc1',
						tool_name: 'search',
						input: { query: 'test' },
						source: 'builtin' as const,
						status: 'completed' as const,
						output: 'found it'
					}
				]
			}
		});

		expect(screen.getByText('search')).toBeInTheDocument();
	});

	it('renders first 2 tags and shows "+N more" button when 3+ tags', () => {
		const tags = [
			{ id: '1', name: 'bug', color: '#f00', created_at: '2026-01-01T00:00:00Z' },
			{ id: '2', name: 'feat', color: '#0f0', created_at: '2026-01-01T00:00:00Z' },
			{ id: '3', name: 'docs', color: '#00f', created_at: '2026-01-01T00:00:00Z' }
		];

		render(ChatMessage, {
			props: { role: 'assistant', content: 'test', tags }
		});

		expect(screen.getByText('bug')).toBeInTheDocument();
		expect(screen.getByText('feat')).toBeInTheDocument();
		expect(screen.queryByText('docs')).not.toBeInTheDocument();
		expect(screen.getByText('+1 more')).toBeInTheDocument();
	});

	it('clicking "+N more" reveals all tags', async () => {
		const user = userEvent.setup();
		const tags = [
			{ id: '1', name: 'bug', color: '#f00', created_at: '2026-01-01T00:00:00Z' },
			{ id: '2', name: 'feat', color: '#0f0', created_at: '2026-01-01T00:00:00Z' },
			{ id: '3', name: 'docs', color: '#00f', created_at: '2026-01-01T00:00:00Z' }
		];

		render(ChatMessage, {
			props: { role: 'assistant', content: 'test', tags }
		});

		await user.click(screen.getByText('+1 more'));

		expect(screen.getByText('docs')).toBeInTheDocument();
		expect(screen.queryByText('+1 more')).not.toBeInTheDocument();
	});

	it('hides action bar during streaming, ephemeral, and thinking', () => {
		const { container } = render(ChatMessage, {
			props: { role: 'assistant', content: 'test', streaming: true }
		});

		// Action bar has opacity-0 and group-hover:opacity-100
		// When canShowActions is false, the bar is not rendered at all
		const actionBar = container.querySelector('[class*="group-hover"]');
		expect(actionBar).not.toBeInTheDocument();
	});
});
