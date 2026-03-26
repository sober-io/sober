import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { userEvent } from '@testing-library/user-event';
import ToolCallDisplay from './ToolCallDisplay.svelte';

describe('ToolCallDisplay', () => {
	it('shows tool name', () => {
		render(ToolCallDisplay, {
			props: { toolName: 'read_file', input: { path: '/tmp/test.txt' } }
		});

		expect(screen.getByText('read_file')).toBeInTheDocument();
	});

	it('shows spinner when loading', () => {
		const { container } = render(ToolCallDisplay, {
			props: { toolName: 'search', input: {}, loading: true }
		});

		const spinner = container.querySelector('.animate-spin');
		expect(spinner).toBeInTheDocument();
	});

	it('shows checkmark icon when completed (not loading, no error)', () => {
		const { container } = render(ToolCallDisplay, {
			props: { toolName: 'search', input: {}, loading: false, isError: false }
		});

		// Checkmark SVG path d="M5 13l4 4L19 7"
		const checkPath = container.querySelector('path[d="M5 13l4 4L19 7"]');
		expect(checkPath).toBeInTheDocument();
	});

	it('shows error icon and "failed" label when isError', () => {
		const { container } = render(ToolCallDisplay, {
			props: { toolName: 'fetch_url', input: {}, isError: true, error: 'timeout' }
		});

		expect(screen.getByText('failed')).toBeInTheDocument();
		// X icon SVG
		const xPath = container.querySelector('path[d="M6 18L18 6M6 6l12 12"]');
		expect(xPath).toBeInTheDocument();
	});

	it('expands to show formatted input on click', async () => {
		const user = userEvent.setup();
		const { container } = render(ToolCallDisplay, {
			props: { toolName: 'search', input: { query: 'test', limit: 10 } }
		});

		await user.click(screen.getByText('search'));

		// Input section visible
		expect(screen.getByText('Input')).toBeInTheDocument();

		// Keys should be syntax-highlighted (rendered via @html with colored spans)
		const pre = container.querySelector('pre');
		expect(pre).toBeInTheDocument();
		expect(pre?.innerHTML).toContain('"query"');
		expect(pre?.innerHTML).toContain('"test"');
	});

	it('shows output when expanded and output provided', async () => {
		const user = userEvent.setup();
		render(ToolCallDisplay, {
			props: { toolName: 'search', input: {}, output: 'Found 3 results' }
		});

		await user.click(screen.getByText('search'));

		expect(screen.getByText('Output')).toBeInTheDocument();
		expect(screen.getByText('Found 3 results')).toBeInTheDocument();
	});

	it('shows error in red when expanded with error', async () => {
		const user = userEvent.setup();
		const { container } = render(ToolCallDisplay, {
			props: { toolName: 'fetch_url', input: {}, isError: true, error: 'Connection refused' }
		});

		await user.click(screen.getByText('fetch_url'));

		const outputPre = container.querySelectorAll('pre')[1];
		expect(outputPre?.textContent).toBe('Connection refused');
		expect(outputPre?.className).toContain('text-red');
	});

	it('formats nested JSON input with syntax highlighting', async () => {
		const user = userEvent.setup();
		const { container } = render(ToolCallDisplay, {
			props: {
				toolName: 'tool',
				input: { name: 'test', count: 42, active: true, data: null }
			}
		});

		await user.click(screen.getByText('tool'));

		const pre = container.querySelector('pre');
		const html = pre?.innerHTML ?? '';

		// Keys highlighted in sky color
		expect(html).toContain('text-sky-600');
		// Strings in emerald
		expect(html).toContain('text-emerald-600');
		// Numbers in amber
		expect(html).toContain('text-amber-600');
		// Booleans in violet
		expect(html).toContain('text-violet-600');
		// Null in zinc
		expect(html).toContain('text-zinc-400');
	});
});
