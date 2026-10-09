// @vitest-environment jsdom
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Api } from '#lib/api/client.ts';
import type { ListEvent, ModuleSummary } from '#lib/api/types.ts';
import { EventStore } from '#lib/events.svelte.ts';
import ListActions from './ListActions.svelte';

const MODULE: ModuleSummary = {
	id: 'd07c9c2425764da8ba056505f57cf40c',
	name: 'MangaDex',
	root_url: 'https://mangadex.org',
	category: 'English',
	capabilities: { account: false, download: true, info: true, update_list: true },
	list_job_running: false,
	list_size: 0,
	list_updated: null,
	option_count: 0
};

const URL = `https://raw.githubusercontent.com/dazedcat19/FMD2-DB/master/7z/${MODULE.id}.7z`;

function failed(reason: ListEvent['reason'], error: string): ListEvent {
	return {
		module_id: MODULE.id,
		job: 'import_db',
		kind: 'failed',
		status_text: '',
		done: 0,
		total: 0,
		titles: null,
		error,
		reason
	};
}

function setup(event: ListEvent) {
	const store = new EventStore({ url: '/api/events', connect: () => ({}) as never });
	store.lists[MODULE.id] = event;
	const updateList = vi.fn(() => Promise.resolve());
	const api = { updateList } as unknown as Api;
	render(ListActions, { api, store, module: MODULE, onfinished: () => {} });
	return { updateList };
}

describe('ListActions', () => {
	it('says FMD2-DB has no list for the website and offers Update list', async () => {
		const { updateList } = setup(
			failed('no_dump', `${MODULE.id}: downloading ${URL} failed with HTTP status 404`)
		);

		const alert = screen.getByRole('alert');
		expect(alert.textContent).toContain(
			'FMD2-DB has no ready-made list for MangaDex. Use Update list to build it from the website.'
		);
		const details = alert.querySelector('details');
		expect(details?.textContent).toContain(URL);
		const outside = [...alert.childNodes].filter((n) => n !== details);
		expect(outside.map((n) => n.textContent).join('')).not.toContain(URL);

		await fireEvent.click(within(alert).getByRole('button', { name: 'Update list' }));
		expect(updateList).toHaveBeenCalledWith(MODULE.id);
	});

	it('words an unreachable FMD2-DB plainly, details aside', () => {
		setup(failed('unreachable', `${MODULE.id}: downloading ${URL} failed with HTTP status 500`));

		const alert = screen.getByRole('alert');
		expect(alert.textContent).toContain('Could not reach FMD2-DB to get the list of MangaDex.');
		expect(alert.querySelector('details')?.textContent).toContain('HTTP status 500');
	});
});
