import { goto } from '$app/navigation';
// eslint-disable-next-line @typescript-eslint/no-unused-vars -- used inside IIFE closure
import { resolve } from '$app/paths';

interface NotifyOptions {
	conversationId: string;
	title: string;
	body: string;
}

/** Desktop notification permission and dispatch. */
export const notifications = (() => {
	/** Request notification permission if not yet decided. */
	const requestPermission = async () => {
		if (typeof Notification === 'undefined') return;
		if (Notification.permission !== 'default') return;
		await Notification.requestPermission();
	};

	/**
	 * Show a desktop notification if the tab is hidden and permission is granted.
	 * Clicking the notification focuses the tab and navigates to the conversation.
	 */
	const notify = ({ conversationId, title, body }: NotifyOptions) => {
		if (typeof Notification === 'undefined') return;
		if (Notification.permission !== 'granted') return;
		if (!document.hidden) return;

		const n = new Notification(title, { body, tag: conversationId });
		n.onclick = () => {
			window.focus();
			// eslint-disable-next-line svelte/no-navigation-without-resolve -- resolve() is called; lint doesn't see through IIFE
			goto(resolve('/(app)/chat/[id]', { id: conversationId }));
			n.close();
		};
	};

	return { requestPermission, notify };
})();
