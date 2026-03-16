import dayjs from 'dayjs';
import relativeTime from 'dayjs/plugin/relativeTime';

dayjs.extend(relativeTime);

export function formatRelativeTime(dateStr: string): string {
	return dayjs(dateStr).fromNow();
}

export function formatRelativeFuture(dateStr: string): string {
	return dayjs(dateStr).fromNow();
}
