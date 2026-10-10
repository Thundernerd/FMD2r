import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = join(import.meta.dirname, '..');

/** Where "FMD2-DB" may stay: the upstream URL, and Settings' one note on where the lists come
 * from (docs/tickets/T85-rename-fmd2-db.md). */
const ALLOWED = [/dazedcat19\/FMD2-DB/g, /from the FMD2-DB project by default/gi];

function files(dir: string): string[] {
	return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
		e.isDirectory() ? files(join(dir, e.name)) : [join(dir, e.name)]
	);
}

describe('user-facing text', () => {
	it('calls FMD2-DB lists ready-made lists', () => {
		const named = files(SRC)
			.filter((f) => !f.endsWith('.test.ts'))
			.filter((f) =>
				ALLOWED.reduce((s, re) => s.replace(re, ''), readFileSync(f, 'utf8')).includes('FMD2-DB')
			)
			.map((f) => relative(SRC, f));

		expect(named).toEqual([]);
	});
});
