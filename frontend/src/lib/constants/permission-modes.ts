import type { PermissionMode } from '$lib/types';

export const PERMISSION_MODES: ReadonlyArray<{
	value: PermissionMode;
	label: string;
	description: string;
	color: string;
}> = [
	{
		value: 'interactive',
		label: 'Interactive',
		description: 'Ask before actions',
		color: 'emerald'
	},
	{ value: 'policy_based', label: 'Policy', description: 'Follow rules', color: 'amber' },
	{ value: 'autonomous', label: 'Autonomous', description: 'Act freely', color: 'red' }
];
