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

/**
 * Draws a red badge with a count onto the favicon, Discord-style.
 * Call with 0 to restore the original favicon.
 */
export const faviconBadge = (() => {
	let originalHref: string | null = null;
	let img: HTMLImageElement | null = null;

	const SIZE = 32;
	const BADGE_RADIUS = 10;

	const ensureOriginal = (): Promise<HTMLImageElement> => {
		if (img) return Promise.resolve(img);

		return new Promise((resolve) => {
			const link = document.querySelector<HTMLLinkElement>('link[rel="icon"]');
			if (!link) return;
			originalHref = link.href;

			const image = new Image();
			image.crossOrigin = 'anonymous';
			image.onload = () => {
				img = image;
				resolve(image);
			};
			image.src = originalHref;
		});
	};

	const update = async (count: number) => {
		if (typeof document === 'undefined') return;

		const link = document.querySelector<HTMLLinkElement>('link[rel="icon"]');
		if (!link) return;

		if (count <= 0) {
			if (originalHref) link.href = originalHref;
			return;
		}

		const image = await ensureOriginal();
		const canvas = document.createElement('canvas');
		canvas.width = SIZE;
		canvas.height = SIZE;
		const ctx = canvas.getContext('2d')!;

		// Draw original favicon
		ctx.drawImage(image, 0, 0, SIZE, SIZE);

		// Draw red badge circle (bottom-right)
		const cx = SIZE - BADGE_RADIUS;
		const cy = SIZE - BADGE_RADIUS;
		ctx.beginPath();
		ctx.arc(cx, cy, BADGE_RADIUS, 0, Math.PI * 2);
		ctx.fillStyle = '#ef4444';
		ctx.fill();

		// Draw count text
		const label = count > 99 ? '99+' : String(count);
		ctx.fillStyle = '#ffffff';
		ctx.font = `bold ${label.length > 2 ? 8 : 11}px sans-serif`;
		ctx.textAlign = 'center';
		ctx.textBaseline = 'middle';
		ctx.fillText(label, cx, cy);

		link.href = canvas.toDataURL('image/png');
	};

	return { update };
})();
