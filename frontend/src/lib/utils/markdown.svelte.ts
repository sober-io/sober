import DOMPurify from 'isomorphic-dompurify';
import { Marked, type Tokens } from 'marked';
import { createHighlighter, type Highlighter } from 'shiki';

// --- Shiki highlighter (async init, sync use) ---

let highlighter: Highlighter | null = null;

/**
 * Reactive version counter — bumps when shiki finishes loading so
 * `$derived` blocks that read `highlighterReady.version` re-render.
 */
export const highlighterReady = (() => {
	let version = $state(0);
	return {
		get version() {
			return version;
		},
		bump() {
			version++;
		}
	};
})();

const SHIKI_LANGS = [
	'typescript',
	'javascript',
	'rust',
	'python',
	'bash',
	'json',
	'html',
	'css',
	'svelte',
	'sql',
	'toml',
	'yaml',
	'markdown',
	'diff',
	'go',
	'shell'
] as const;

const SHIKI_LANG_ALIASES: Record<string, string> = {
	ts: 'typescript',
	js: 'javascript',
	rs: 'rust',
	py: 'python',
	sh: 'bash',
	zsh: 'bash',
	yml: 'yaml',
	md: 'markdown',
	svx: 'svelte'
};

/** Return the shiki highlighter instance if loaded, or null. */
export function getHighlighter(): Highlighter | null {
	return highlighter;
}

/** Load shiki highlighter in the background. Until loaded, code renders unstyled. */
if (typeof window !== 'undefined') {
	createHighlighter({
		themes: ['github-dark'],
		langs: [...SHIKI_LANGS]
	}).then((h) => {
		highlighter = h;
		highlighterReady.bump();
	});
}

function resolveLanguage(lang: string | undefined): string | undefined {
	if (!lang) return undefined;
	const lower = lang.toLowerCase();
	return SHIKI_LANG_ALIASES[lower] ?? (SHIKI_LANGS.includes(lower as never) ? lower : undefined);
}

// --- Marked with custom code renderer ---

const renderer = {
	code({ text, lang }: Tokens.Code): string {
		const resolved = resolveLanguage(lang);

		if (highlighter && resolved) {
			return highlighter.codeToHtml(text, {
				lang: resolved,
				theme: 'github-dark'
			});
		}

		// Fallback: plain code block with language class
		const escaped = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
		const langClass = resolved ? ` class="language-${resolved}"` : '';
		return `<pre class="shiki"><code${langClass}>${escaped}</code></pre>`;
	}
};

const marked = new Marked({
	async: false,
	gfm: true,
	breaks: true,
	renderer
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
	ALLOWED_ATTR: ['href', 'src', 'alt', 'title', 'class', 'id', 'target', 'rel', 'style'],
	ALLOW_DATA_ATTR: false,
	ADD_ATTR: ['target'],
	FORBID_TAGS: ['style', 'script', 'iframe', 'object', 'embed', 'form', 'input', 'textarea']
};

/**
 * Close unclosed code fences so partial streaming content renders correctly
 * instead of showing raw backticks.
 */
function closeUnmatchedCodeFences(content: string): string {
	const fenceRegex = /^(`{3,})/gm;
	let openFence: string | null = null;
	let match;

	while ((match = fenceRegex.exec(content)) !== null) {
		if (openFence) {
			// Closing fence must be at least as long as opening
			if (match[1].length >= openFence.length) {
				openFence = null;
			}
		} else {
			openFence = match[1];
		}
	}

	if (openFence) {
		return content + '\n' + openFence;
	}
	return content;
}

/** Parse markdown to HTML with XSS sanitization via DOMPurify. */
export function renderMarkdown(content: string): string {
	const processed = closeUnmatchedCodeFences(content);
	const html = marked.parse(processed) as string;
	return DOMPurify.sanitize(html, PURIFY_CONFIG) as string;
}
