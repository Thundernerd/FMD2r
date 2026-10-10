import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

/** The style tokens, and the themes and accents the settings offer (openapi.json). */
const TOKENS = readFileSync(new URL('./tokens.css', import.meta.url), 'utf8');
const schemas = (
	JSON.parse(readFileSync(new URL('../../../../openapi.json', import.meta.url), 'utf8')) as {
		components: { schemas: Record<string, { enum?: string[] }> };
	}
).components.schemas;
const THEMES = schemas['Theme']?.enum ?? [];
const ACCENTS = schemas['Accent']?.enum ?? [];
const MODES = ['light', 'dark'] as const;

/** WCAG's minimum contrast for normal text (AA), and AAA for the high-contrast theme. */
const minimum = (theme: string) => (theme === 'high-contrast' ? 7 : 4.5);

interface Rule {
	selectors: string[];
	declarations: Map<string, string>;
}

/** The top-level rules of a stylesheet, without at-rules such as `@media`. */
function rules(css: string): Rule[] {
	const text = css.replace(/\/\*[\s\S]*?\*\//g, '');
	const found: Rule[] = [];
	let depth = 0;
	let start = 0;
	let prelude = '';
	for (let i = 0; i < text.length; i++) {
		if (text[i] === '{') {
			if (depth === 0) {
				prelude = text.slice(start, i).trim();
				start = i + 1;
			}
			depth++;
		} else if (text[i] === '}') {
			depth--;
			if (depth === 0) {
				if (!prelude.startsWith('@')) {
					found.push({
						selectors: prelude.split(',').map((s) => s.trim()),
						declarations: declarations(text.slice(start, i))
					});
				}
				start = i + 1;
			}
		}
	}
	return found;
}

/** The custom properties a rule sets. */
function declarations(body: string): Map<string, string> {
	const result = new Map<string, string>();
	for (const m of body.matchAll(/(--[\w-]+)\s*:\s*((?:[^;()]|\((?:[^()]|\([^()]*\))*\))*);/g)) {
		const [, name, value] = m;
		if (name && value) result.set(name, value.trim());
	}
	return result;
}

/**
 * The specificity of a selector made of `:root` and attribute tests, when it matches `<html>` with
 * these attributes; `null` when it doesn't.
 */
function match(selector: string, attrs: Record<string, string>): number | null {
	const parts = selector.match(/:root|\[[\w-]+='[^']*'\]/g) ?? [];
	if (parts.join('') !== selector.replace(/\s+/g, '')) return null;
	for (const part of parts) {
		const attr = part.match(/^\[([\w-]+)='([^']*)'\]$/);
		if (attr && attrs[attr[1] ?? ''] !== attr[2]) return null;
	}
	return parts.length;
}

/** The tokens on `<html>` with these attributes, in the cascade's order. */
function tokens(attrs: Record<string, string>): Map<string, string> {
	const applied = rules(TOKENS)
		.map((rule, order) => {
			const specificity = Math.max(
				...rule.selectors.map((s) => match(s, attrs) ?? Number.NEGATIVE_INFINITY)
			);
			return { rule, order, specificity };
		})
		.filter((r) => r.specificity > Number.NEGATIVE_INFINITY)
		.sort((a, b) => a.specificity - b.specificity || a.order - b.order);
	const result = new Map<string, string>();
	for (const { rule } of applied) for (const [k, v] of rule.declarations) result.set(k, v);
	return result;
}

/** The colour a token has in a mode: one side of `light-dark()`, or the same in both. */
function colour(value: string | undefined, mode: (typeof MODES)[number]): string {
	if (!value) throw new Error('missing token');
	const pair = value.match(/^light-dark\(\s*(#[0-9a-f]{6})\s*,\s*(#[0-9a-f]{6})\s*\)$/i);
	if (pair) return (mode === 'light' ? pair[1] : pair[2]) ?? '';
	if (/^#[0-9a-f]{6}$/i.test(value)) return value;
	throw new Error(`not a colour pair: ${value}`);
}

/** WCAG 2 contrast ratio of two `#rrggbb` colours. */
function contrast(a: string, b: string): number {
	const luminance = (hex: string) => {
		const [r = 0, g = 0, bl = 0] = [1, 3, 5].map((i) => {
			const c = parseInt(hex.slice(i, i + 2), 16) / 255;
			return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
		});
		return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
	};
	const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
	return ((hi ?? 0) + 0.05) / ((lo ?? 0) + 0.05);
}

const PAIRS: [fg: string, bg: string][] = [
	['--fg', '--bg'],
	['--fg', '--surface'],
	['--muted', '--surface'],
	['--accent-fg', '--accent']
];

describe('themes', () => {
	it('are the four built-in ones', () => {
		expect(THEMES).toEqual(['default', 'high-contrast', 'warm', 'compact']);
		expect(ACCENTS.length).toBeGreaterThan(1);
	});

	it.each(THEMES.filter((t) => t !== 'default'))('%s has its own block of tokens', (theme) => {
		const own = rules(TOKENS).filter((r) =>
			r.selectors.some((s) => s.includes(`[data-style='${theme}']`))
		);
		expect(own.length).toBeGreaterThan(0);
	});

	const cases = THEMES.flatMap((theme) =>
		MODES.flatMap((mode) => ACCENTS.map((accent) => [theme, mode, accent] as const))
	);
	it.each(cases)(
		'%s in %s with the %s accent meets its contrast minimum',
		(theme, mode, accent) => {
			const t = tokens({ 'data-style': theme, 'data-accent': accent });
			const low = PAIRS.map(([fg, bg]) => {
				const ratio = contrast(colour(t.get(fg), mode), colour(t.get(bg), mode));
				return { pair: `${fg} on ${bg}`, ratio: Math.round(ratio * 100) / 100 };
			}).filter(({ ratio }) => ratio < minimum(theme));
			expect(low).toEqual([]);
		}
	);

	it.each(MODES)('high contrast draws its borders at 7:1 too, in %s', (mode) => {
		const t = tokens({ 'data-style': 'high-contrast', 'data-accent': 'teal' });
		const line = colour(t.get('--line'), mode);
		for (const bg of ['--bg', '--surface', '--surface-2']) {
			expect(contrast(line, colour(t.get(bg), mode)), bg).toBeGreaterThanOrEqual(7);
		}
	});

	it('keep the 7:1 accent of high contrast whichever swatch is saved', () => {
		const accents = new Set(
			ACCENTS.map((accent) =>
				tokens({ 'data-style': 'high-contrast', 'data-accent': accent }).get('--accent')
			)
		);
		expect(accents.size).toBe(1);
	});
});
