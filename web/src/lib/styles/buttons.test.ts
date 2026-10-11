import { readFileSync, readdirSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

/** The shared button styles every `.btn` uses (issue #226). */
const APP = readFileSync(new URL('./app.css', import.meta.url), 'utf8').replace(
	/\/\*[\s\S]*?\*\//g,
	''
);

/** The declarations of the top-level rules whose selector list contains `selector`. */
function declarations(selector: string): Map<string, string> {
	const result = new Map<string, string>();
	for (const [, prelude = '', body = ''] of APP.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
		if (!prelude.split(',').some((s) => s.trim() === selector)) continue;
		for (const [, name = '', value = ''] of body.matchAll(/([\w-]+)\s*:\s*([^;]+);/g)) {
			result.set(name, value.trim());
		}
	}
	return result;
}

/** Every component under `src/`, with its path. */
function components(dir: string): string[] {
	return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
		const path = join(dir, entry.name);
		if (entry.isDirectory()) return components(path);
		return entry.name.endsWith('.svelte') ? [path] : [];
	});
}

describe('disabled buttons', () => {
	it('are faded and show a not-allowed cursor', () => {
		const disabled = declarations('.btn:disabled');
		expect(Number(disabled.get('opacity'))).toBeLessThanOrEqual(0.5);
		expect(disabled.get('cursor')).toBe('not-allowed');
	});

	it('only highlight on hover when enabled', () => {
		expect(declarations('.btn:hover').has('border-color')).toBe(false);
		expect(declarations('.btn:hover:not(:disabled)').get('border-color')).toBe('var(--accent)');
	});

	it('look the same in every component', () => {
		const src = join(import.meta.dirname, '../..');
		const own = components(src)
			.filter((file) => /\.btn[^{},]*:disabled/.test(readFileSync(file, 'utf8')))
			.map((file) => relative(src, file));
		expect(own).toEqual([]);
	});
});
