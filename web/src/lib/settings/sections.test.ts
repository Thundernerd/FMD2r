import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { createMockBackend } from '#lib/api/mock.ts';
import { createApi } from '#lib/api/client.ts';
import { SETTINGS_SECTIONS } from '#lib/settings/sections.ts';

// The server's OpenAPI document carries the T18 defaults of every settings group.
const openapi = JSON.parse(
	readFileSync(new URL('../../../../openapi.json', import.meta.url), 'utf8')
) as {
	components: { schemas: { Settings: { properties: Record<string, { default: unknown }> } } };
};
const DEFAULTS = Object.fromEntries(
	Object.entries(openapi.components.schemas.Settings.properties).map(([k, v]) => [k, v.default])
);

/** Dotted paths of every leaf setting under `value`. */
function leaves(value: unknown, prefix = ''): string[] {
	if (typeof value !== 'object' || value === null || Array.isArray(value)) return [prefix];
	return Object.entries(value).flatMap(([k, v]) => leaves(v, prefix ? `${prefix}.${k}` : k));
}

describe('settings sections', () => {
	it('have a control for every setting of the T18 model, once', () => {
		const paths = SETTINGS_SECTIONS.flatMap((s) => s.fields.map((f) => f.path));
		expect([...paths].sort()).toEqual(leaves(DEFAULTS).sort());
	});

	it('mirror the T18 groups in the table of contents', () => {
		const groups = new Set(
			SETTINGS_SECTIONS.flatMap((s) => s.fields.map((f) => f.path.split('.')[0]))
		);
		expect([...groups].sort()).toEqual(Object.keys(DEFAULTS).sort());
	});

	it('offer every value of each enum setting', () => {
		const schemas = openapi.components.schemas as unknown as Record<string, { enum?: string[] }>;
		const select = (path: string) =>
			SETTINGS_SECTIONS.flatMap((s) => s.fields).find((f) => f.path === path)?.control;
		const values = (path: string) => {
			const control = select(path);
			return control?.kind === 'select' ? control.choices.map((c) => c.value) : [];
		};
		expect(values('output.format')).toEqual(schemas['OutputFormat']?.enum);
		expect(values('images.webp_save_as')).toEqual(schemas['WebpSaveAs']?.enum);
		expect(values('images.png_compression')).toEqual(schemas['PngCompression']?.enum);
		expect(values('connections.proxy.type')).toEqual(schemas['ProxyType']?.enum);
		expect(values('saveto.illegal_chars')).toEqual(schemas['SymbolMode']?.enum);
		expect(values('xpath.backend')).toEqual(schemas['XPathBackend']?.enum);
	});

	it('the mock backend starts from the same defaults as the server', async () => {
		const api = createApi({ baseUrl: 'http://fmd2r.test', fetch: createMockBackend().fetch });
		expect(await api.getSettings()).toEqual(DEFAULTS);
	});
});
