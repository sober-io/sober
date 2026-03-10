import type { Conversation } from '$lib/types';

/** Shared reactive conversations list for sidebar and chat pages. */
export const conversations = (() => {
	let items = $state<Conversation[]>([]);
	let loading = $state(true);

	return {
		get items() {
			return items;
		},
		get loading() {
			return loading;
		},
		set(list: Conversation[]) {
			items = list;
			loading = false;
		},
		prepend(conv: Conversation) {
			items = [conv, ...items];
		},
		updateTitle(id: string, title: string) {
			const conv = items.find((c) => c.id === id);
			if (conv) conv.title = title;
		},
		remove(id: string) {
			items = items.filter((c) => c.id !== id);
		}
	};
})();
