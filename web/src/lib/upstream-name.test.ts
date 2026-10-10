import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = join(import.meta.dirname, '..');

/** Where "FMD2-DB" may stay: the upstream URL, and Settings' one note on where the lists come
 * from (docs/tickets/T85-rename-fmd2-db.md). */
const ALLOWED: [RegExp, string[]][] = [
	[/dazedcat19\/FMD2-DB/g, []],
	// The setting's help, and its description in the generated API schema.
	[/from the FMD2-DB project by default/gi, ['lib/settings/sections.ts', 'lib/api/schema.d.ts']]
];

function unnamed(file: string): string {
	return ALLOWED.reduce(
		(text, [re, only]) => (only.length && !only.includes(file) ? text : text.replace(re, '')),
		readFileSync(join(SRC, file), 'utf8')
	);
}

function files(dir: string): string[] {
	return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
		e.isDirectory() ? files(join(dir, e.name)) : [join(dir, e.name)]
	);
}

describe('user-facing text', () => {
	it('calls FMD2-DB lists ready-made lists', () => {
		const named = files(SRC)
			.filter((f) => !f.endsWith('.test.ts'))
			.map((f) => relative(SRC, f))
			.filter((f) => unnamed(f).includes('FMD2-DB'));

		expect(named).toEqual([]);
	});
});
