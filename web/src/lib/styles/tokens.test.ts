import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = join(import.meta.dirname, '../..');

/** The only file that may spell out a font size or a colour; everything else uses its tokens, so
 * overriding them restyles the whole app (docs/tickets/T88-style-tokens-everywhere.md). */
const TOKENS = 'lib/styles/tokens.css';

const HARD_CODED: [string, RegExp][] = [
	['a font size in px', /font-size\s*:\s*[\d.]+px/gi],
	['a font shorthand with a size in px', /\bfont\s*:[^;]*?[\d.]+px/gi],
	['a hex colour', /#[0-9a-f]{3,8}\b/gi],
	['an rgb() colour', /\brgba?\s*\(/gi],
	// `hsl(var(--h) var(--tone))` is fine: the hue is per series, the tone a token.
	['an hsl() colour with a literal tone', /\bhsla?\s*\((?:[^()]|\([^()]*\))*?\d%/gi]
];

function styled(dir: string): string[] {
	return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
		const path = join(dir, entry.name);
		if (entry.isDirectory()) return styled(path);
		return /\.(svelte|css)$/.test(entry.name) ? [relative(SRC, path)] : [];
	});
}

/** The CSS in a file: all of a stylesheet, or a component's `<style>` blocks and `style` attributes. */
function css(file: string): string {
	const text = readFileSync(join(SRC, file), 'utf8');
	if (file.endsWith('.css')) return text;
	const blocks = [...text.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)].map((m) => m[1]);
	const attrs = [...text.matchAll(/\sstyle(?::[\w-]+)?=(["'])([\s\S]*?)\1/g)].map((m) => m[2]);
	return [...blocks, ...attrs].join('\n');
}

describe('style tokens', () => {
	const files = styled(SRC).filter((file) => file !== TOKENS);

	it('finds the stylesheets and components to check', () => {
		expect(files).toContain('lib/styles/app.css');
		expect(files).toContain('routes/+page.svelte');
	});

	it.each(files)('%s takes its font sizes and colours from tokens', (file) => {
		const text = css(file);
		const found = HARD_CODED.flatMap(([what, re]) =>
			[...text.matchAll(re)].map((m) => `${what}: ${m[0]}`)
		);
		expect(found).toEqual([]);
	});
});
