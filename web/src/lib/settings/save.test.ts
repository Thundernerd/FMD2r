import { describe, expect, it } from 'vitest';
import { createApi, ValidationError } from '#lib/api/client.ts';
import { createMockBackend } from '#lib/api/mock.ts';
import { Draft } from '#lib/settings/draft.svelte.ts';
import { showFieldErrors } from '#lib/settings/save.ts';

const api = () => createApi({ baseUrl: 'http://fmd2r.test', fetch: createMockBackend().fetch });

describe('saving settings and module settings together', () => {
	it('reports every invalid field of a rejected save', async () => {
		const error = await api()
			.patchAllSettings({
				settings: {
					connections: { max_parallel_tasks: 0, timeout_secs: 0 },
					general: { language: 'nl' }
				},
				modules: { mangadex: { options: { language: 99 } } }
			})
			.catch((e: unknown) => e);

		expect(error).toBeInstanceOf(ValidationError);
		const fields = (error as ValidationError).fields.map((f) => f.field).sort();
		expect(fields).toEqual([
			'modules.mangadex.options.language',
			'settings.connections.max_parallel_tasks',
			'settings.connections.timeout_secs'
		]);
	});

	it('leaves both unchanged when one part is invalid', async () => {
		const client = api();
		await client
			.patchAllSettings({
				settings: { general: { add_as_stopped: true } },
				modules: { mangadex: { options: { language: 99 } } }
			})
			.catch(() => undefined);
		expect((await client.getSettings()).general.add_as_stopped).toBe(false);
	});

	it('shows each error next to its field in the draft it belongs to', () => {
		const settings = new Draft({ connections: { max_parallel_tasks: 0, timeout_secs: 0 } });
		const module = new Draft({ options: { language: 99 } });

		const unplaced = showFieldErrors(
			[
				{ field: 'settings.connections.max_parallel_tasks', detail: '0 is outside 1..=64' },
				{ field: 'settings.connections.timeout_secs', detail: '0 is outside 1..=300' },
				{ field: 'modules.mangadex.options.language', detail: 'expected the index of an item' },
				{ field: 'modules.other.enabled', detail: 'not shown' }
			],
			settings,
			{ id: 'mangadex', draft: module }
		);

		expect(settings.errors).toEqual({
			'connections.max_parallel_tasks': '0 is outside 1..=64',
			'connections.timeout_secs': '0 is outside 1..=300'
		});
		expect(module.errors).toEqual({ 'options.language': 'expected the index of an item' });
		expect(unplaced).toEqual(['modules.other.enabled: not shown']);
	});
});
