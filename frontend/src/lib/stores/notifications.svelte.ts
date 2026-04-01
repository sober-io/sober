import { goto } from '$app/navigation';

interface NotifyOptions {
	conversationId: string;
	title: string;
	body: string;
	/** Whether this conversation is currently being viewed. */
	isActiveConversation?: boolean;
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
			goto(`/chat/${conversationId}`);
			n.close();
		};
	};

	return { requestPermission, notify };
})();
