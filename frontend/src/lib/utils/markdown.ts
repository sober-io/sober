import DOMPurify from 'isomorphic-dompurify';
import { Marked } from 'marked';

const marked = new Marked({
	async: false,
	gfm: true,
	breaks: true
});

/** Allowed HTML tags after markdown rendering. */
const PURIFY_CONFIG = {
	ALLOWED_TAGS: [
		// Inline
		'b',
		'i',
		'em',
		'strong',
		'code',
		'kbd',
		'mark',
		'del',
		's',
		'sub',
		'sup',
		'br',
		'span',
		'a',
		// Block
		'p',
		'div',
		'h1',
		'h2',
		'h3',
		'h4',
		'h5',
		'h6',
		'blockquote',
		'pre',
		'hr',
		// Lists
		'ul',
		'ol',
		'li',
		// Tables
		'table',
		'thead',
		'tbody',
		'tr',
		'th',
		'td',
		// Media (src validated by DOMPurify)
		'img'
	],
	ALLOWED_ATTR: ['href', 'src', 'alt', 'title', 'class', 'id', 'target', 'rel'],
	ALLOW_DATA_ATTR: false,
	ADD_ATTR: ['target'],
	FORBID_TAGS: ['style', 'script', 'iframe', 'object', 'embed', 'form', 'input', 'textarea']
};

/** Parse markdown to HTML with XSS sanitization via DOMPurify. */
export function renderMarkdown(content: string): string {
	const html = marked.parse(content) as string;
	return DOMPurify.sanitize(html, PURIFY_CONFIG) as string;
}
