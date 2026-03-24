import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import { userEvent } from '@testing-library/user-event';
import RegisterPage from './+page.svelte';

// Mock the services
vi.mock('$lib/services/system', () => ({
	systemService: {
		status: vi.fn()
	}
}));

vi.mock('$lib/services/auth', () => ({
	authService: {
		register: vi.fn()
	}
}));

vi.mock('$app/paths', () => ({
	resolve: (path: string) => path
}));

import { systemService } from '$lib/services/system';
import { authService } from '$lib/services/auth';

const mockStatus = systemService.status as ReturnType<typeof vi.fn>;
const mockRegister = authService.register as ReturnType<typeof vi.fn>;

describe('Register page', () => {
	beforeEach(() => {
		vi.clearAllMocks();
	});

	it('shows welcome text when system is not initialized', async () => {
		mockStatus.mockResolvedValue({ initialized: false });
		render(RegisterPage);

		await waitFor(() => {
			expect(
				screen.getByText('Welcome to Sober! Create your admin account to get started.')
			).toBeInTheDocument();
		});
		expect(screen.getByText('Create admin account')).toBeInTheDocument();
		expect(screen.getByRole('button', { name: 'Create Admin Account' })).toBeInTheDocument();
	});

	it('shows normal form when system is initialized', async () => {
		mockStatus.mockResolvedValue({ initialized: true });
		render(RegisterPage);

		await waitFor(() => {
			expect(screen.getByRole('heading', { name: 'Create account' })).toBeInTheDocument();
		});
		expect(
			screen.queryByText('Welcome to Sober! Create your admin account to get started.')
		).not.toBeInTheDocument();
		expect(screen.getByRole('button', { name: 'Create account' })).toBeInTheDocument();
	});

	it('shows sign-in link after first user registration', async () => {
		mockStatus.mockResolvedValue({ initialized: false });
		mockRegister.mockResolvedValue({
			id: '1',
			email: 'admin@example.com',
			username: 'admin',
			status: 'Active'
		});

		const user = userEvent.setup();
		render(RegisterPage);

		await waitFor(() => {
			expect(screen.getByText('Create admin account')).toBeInTheDocument();
		});

		await user.type(screen.getByLabelText('Email'), 'admin@example.com');
		await user.type(screen.getByLabelText('Username'), 'admin');
		await user.type(screen.getByLabelText('Password'), 'securepassword123');
		await user.click(screen.getByRole('button', { name: 'Create Admin Account' }));

		await waitFor(() => {
			expect(screen.getByText('Account created!')).toBeInTheDocument();
		});
		expect(screen.getByText('You can now sign in.')).toBeInTheDocument();
		expect(screen.getByRole('link', { name: 'Sign in' })).toHaveAttribute('href', '/login');
	});

	it('shows pending message after normal registration', async () => {
		mockStatus.mockResolvedValue({ initialized: true });
		mockRegister.mockResolvedValue({
			id: '2',
			email: 'user@example.com',
			username: 'newuser',
			status: 'Pending'
		});

		const user = userEvent.setup();
		render(RegisterPage);

		await waitFor(() => {
			expect(screen.getByRole('heading', { name: 'Create account' })).toBeInTheDocument();
		});

		await user.type(screen.getByLabelText('Email'), 'user@example.com');
		await user.type(screen.getByLabelText('Username'), 'newuser');
		await user.type(screen.getByLabelText('Password'), 'securepassword123');
		await user.click(screen.getByRole('button', { name: 'Create account' }));

		await waitFor(() => {
			expect(screen.getByText('Registration submitted')).toBeInTheDocument();
		});
		expect(screen.getByText(/pending approval/)).toBeInTheDocument();
	});
});
