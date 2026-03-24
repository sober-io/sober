import { describe, it, expect } from 'vitest';
import { renderMarkdown } from './markdown';

describe('renderMarkdown', () => {
	it('renders basic markdown formatting', () => {
		const html = renderMarkdown('**bold** _italic_ `code`');

		expect(html).toContain('<strong>bold</strong>');
		expect(html).toContain('<em>italic</em>');
		expect(html).toContain('<code>code</code>');
	});

	it('renders GFM features: tables, strikethrough, line breaks', () => {
		const table = renderMarkdown('| A | B |\n|---|---|\n| 1 | 2 |');
		expect(table).toContain('<table>');
		expect(table).toContain('<td>1</td>');

		const strike = renderMarkdown('~~deleted~~');
		expect(strike).toContain('<del>deleted</del>');
	});

	it('strips script tags (XSS)', () => {
		const html = renderMarkdown('<script>alert("xss")</script>hello');

		expect(html).not.toContain('<script>');
		expect(html).not.toContain('alert');
		expect(html).toContain('hello');
	});

	it('strips forbidden tags: iframe, object, embed, form, style', () => {
		const tests = [
			'<iframe src="evil.com"></iframe>',
			'<object data="x"></object>',
			'<embed src="x">',
			'<form action="/steal"><input></form>',
			'<style>body{display:none}</style>'
		];

		for (const input of tests) {
			const html = renderMarkdown(input);
			expect(html).not.toMatch(/<(iframe|object|embed|form|input|style)/);
		}
	});

	it('preserves allowed tags: tables, images', () => {
		const img = renderMarkdown('![alt text](https://example.com/img.png "title")');
		expect(img).toContain('<img');
		expect(img).toContain('src="https://example.com/img.png"');
		expect(img).toContain('alt="alt text"');
	});

	it('blocks data-* attributes', () => {
		const html = renderMarkdown('<div data-evil="payload">content</div>');

		expect(html).not.toContain('data-evil');
		expect(html).toContain('content');
	});
});
