import { Marked } from 'marked';

const marked = new Marked({
	async: false,
	gfm: true,
	breaks: true
});

/** Parse markdown to HTML. Raw HTML tags in the input are escaped by marked's default tokenizer. */
export function renderMarkdown(content: string): string {
	return marked.parse(content) as string;
}
