import { describe, expect, it } from 'vitest';
import { Draft } from '#lib/settings/draft.svelte.ts';

const saved = () => ({
	connections: { max_parallel_tasks: 1, proxy: { host: '', port: null as number | null } },
	general: { language: 'en' }
});

describe('settings draft dirty tracking', () => {
	it('starts clean and reads values by path', () => {
		const draft = new Draft(saved());
		expect(draft.dirty).toBe(false);
		expect(draft.changes()).toBeNull();
		expect(draft.get('connections.proxy.host')).toBe('');
	});

	it('turns dirty on an edit and reports only the changed leaves as a merge patch', () => {
		const draft = new Draft(saved());
		draft.set('connections.max_parallel_tasks', 4);
		draft.set('connections.proxy.port', 8080);

		expect(draft.dirty).toBe(true);
		expect(draft.isDirty('connections.max_parallel_tasks')).toBe(true);
		expect(draft.isDirty('general.language')).toBe(false);
		expect(draft.changes()).toEqual({
			connections: { max_parallel_tasks: 4, proxy: { port: 8080 } }
		});
	});

	it('is clean again when an edit is undone by hand', () => {
		const draft = new Draft(saved());
		draft.set('general.language', 'de');
		draft.set('general.language', 'en');
		expect(draft.dirty).toBe(false);
		expect(draft.changes()).toBeNull();
	});

	it('sends a cleared nullable value as null', () => {
		const draft = new Draft({ proxy: { port: 8080 as number | null } });
		draft.set('proxy.port', null);
		expect(draft.changes()).toEqual({ proxy: { port: null } });
	});

	it('reset drops the edits; commit adopts the saved state and clears errors', () => {
		const draft = new Draft(saved());
		draft.set('general.language', 'de');
		draft.reset();
		expect(draft.get('general.language')).toBe('en');
		expect(draft.dirty).toBe(false);

		draft.set('connections.max_parallel_tasks', 0);
		draft.errors['connections.max_parallel_tasks'] = '0 is outside 1..=64';
		const server = saved();
		server.connections.max_parallel_tasks = 2;
		draft.commit(server);
		expect(draft.get('connections.max_parallel_tasks')).toBe(2);
		expect(draft.dirty).toBe(false);
		expect(draft.errors).toEqual({});
	});

	it('editing a field clears its error', () => {
		const draft = new Draft(saved());
		draft.errors['general.language'] = 'bad';
		draft.set('general.language', 'fr');
		expect(draft.errors['general.language']).toBeUndefined();
	});

	it('does not share state with the object it was given', () => {
		const initial = saved();
		const draft = new Draft(initial);
		draft.set('general.language', 'de');
		expect(initial.general.language).toBe('en');
	});
});
