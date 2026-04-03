import type { Conversation, Tag } from '$lib/types';

/** Shared reactive conversations list for sidebar and chat pages. */
export const conversations = (() => {
	let items = $state<Conversation[]>([]);
	let loading = $state(false);
	let showArchived = $state(false);
	let inbox = $state<Conversation | null>(null);

	const totalUnread = $derived(items.reduce((sum, c) => sum + (c.unread_count ?? 0), 0));

	return {
		get items() {
			return items;
		},
		get totalUnread() {
			return totalUnread;
		},
		get loading() {
			return loading;
		},
		get showArchived() {
			return showArchived;
		},
		get inbox() {
			return inbox;
		},

		set(list: Conversation[]) {
			items = list;
		},
		setLoading(v: boolean) {
			loading = v;
		},
		setShowArchived(v: boolean) {
			showArchived = v;
		},
		setInbox(conv: Conversation) {
			inbox = conv;
		},

		prepend(conv: Conversation) {
			items = [conv, ...items];
		},
		updateTitle(id: string, title: string) {
			items = items.map((c) => (c.id === id ? { ...c, title } : c));
		},
		remove(id: string) {
			items = items.filter((c) => c.id !== id);
		},

		updateUnread(conversationId: string, unreadCount: number) {
			items = items.map((c) => (c.id === conversationId ? { ...c, unread_count: unreadCount } : c));
			// Re-sort: unread first, then by updated_at
			items.sort((a, b) => {
				if (a.unread_count > 0 && b.unread_count === 0) return -1;
				if (a.unread_count === 0 && b.unread_count > 0) return 1;
				return Date.parse(b.updated_at) - Date.parse(a.updated_at);
			});
		},

		markRead(conversationId: string) {
			items = items.map((c) => (c.id === conversationId ? { ...c, unread_count: 0 } : c));
		},

		archive(id: string) {
			items = items.map((c) => (c.id === id ? { ...c, is_archived: true } : c));
		},

		unarchive(id: string) {
			items = items.map((c) => (c.id === id ? { ...c, is_archived: false } : c));
		},

		updateTags(id: string, tags: Tag[]) {
			items = items.map((c) => (c.id === id ? { ...c, tags } : c));
		},

		update(id: string, fields: Partial<Conversation>) {
			items = items.map((c) => (c.id === id ? { ...c, ...fields } : c));
		}
	};
})();
