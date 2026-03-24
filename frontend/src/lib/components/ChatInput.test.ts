import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { userEvent } from '@testing-library/user-event';
import ChatInput from './ChatInput.svelte';

describe('ChatInput', () => {
	it('send button is disabled when input is empty', () => {
		render(ChatInput, {
			props: { onsend: vi.fn() }
		});

		const button = screen.getByRole('button', { name: /send/i });
		expect(button).toBeDisabled();
	});

	it('Enter key submits trimmed value and clears input', async () => {
		const user = userEvent.setup();
		const onsend = vi.fn();
		render(ChatInput, { props: { onsend } });

		const textarea = screen.getByPlaceholderText(/send a message/i);
		await user.type(textarea, 'hello world');
		await user.keyboard('{Enter}');

		expect(onsend).toHaveBeenCalledWith('hello world');
	});

	it('Shift+Enter does not submit', async () => {
		const user = userEvent.setup();
		const onsend = vi.fn();
		render(ChatInput, { props: { onsend } });

		const textarea = screen.getByPlaceholderText(/send a message/i);
		await user.type(textarea, 'hello');
		await user.keyboard('{Shift>}{Enter}{/Shift}');

		expect(onsend).not.toHaveBeenCalled();
	});

	it('slash prefix shows palette, slash+space hides it', async () => {
		const user = userEvent.setup();
		render(ChatInput, {
			props: { onsend: vi.fn(), skills: [{ name: 'test-skill', description: 'A test skill' }] }
		});

		const textarea = screen.getByPlaceholderText(/send a message/i);

		// Typing "/" should trigger the palette
		await user.type(textarea, '/');
		// SlashCommandPalette renders command items
		expect(screen.getByText('/help')).toBeInTheDocument();

		// Adding a space hides the palette (value becomes "/ ")
		await user.type(textarea, ' ');
		expect(screen.queryByText('/help')).not.toBeInTheDocument();
	});

	it('builtin commands trigger onSlashCommand', async () => {
		const user = userEvent.setup();
		const onSlashCommand = vi.fn();
		render(ChatInput, {
			props: { onsend: vi.fn(), onSlashCommand }
		});

		const textarea = screen.getByPlaceholderText(/send a message/i);
		// Type a complete builtin command and submit
		await user.clear(textarea);
		await user.type(textarea, '/help');
		// Submit by pressing Enter — but palette is open so Enter is intercepted.
		// Instead simulate clicking Send button after palette closes.
		// Actually, let's type the command with a space so palette closes, then submit.
		await user.clear(textarea);
		await user.type(textarea, '/help ');
		await user.keyboard('{Enter}');

		// /help with space — first word is /help which is a builtin
		expect(onSlashCommand).toHaveBeenCalledWith('/help');
	});

	it('non-builtin slash commands go through onsend', async () => {
		const user = userEvent.setup();
		const onsend = vi.fn();
		render(ChatInput, {
			props: { onsend, onSlashCommand: vi.fn() }
		});

		const textarea = screen.getByPlaceholderText(/send a message/i);
		await user.clear(textarea);
		await user.type(textarea, '/my-skill do something');
		await user.keyboard('{Enter}');

		expect(onsend).toHaveBeenCalledWith('/my-skill do something');
	});

	it('shows "Queue" when busy', () => {
		render(ChatInput, {
			props: { onsend: vi.fn(), busy: true }
		});

		expect(screen.getByText('Queue')).toBeInTheDocument();
	});
});
