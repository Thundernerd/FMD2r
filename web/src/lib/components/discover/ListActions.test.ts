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
	customized: false,
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

function setup(event: ListEvent | undefined, module: ModuleSummary = MODULE) {
	const store = new EventStore({ url: '/api/events', connect: () => ({}) as never });
	if (event) store.lists[MODULE.id] = event;
	const updateList = vi.fn(() => Promise.resolve());
	const api = { updateList } as unknown as Api;
	const { unmount } = render(ListActions, { api, store, module, onfinished: () => {} });
	return { updateList, unmount };
}

describe('ListActions', () => {
	it('offers a ready-made list when the website has none yet', () => {
		setup(undefined);

		expect(screen.getByRole('button', { name: 'Get ready-made list' })).toBeTruthy();
		expect(document.body.textContent).toContain(
			'No list yet. Get a ready-made one, or build it from the website (slow).'
		);
		expect(document.body.textContent).not.toContain('FMD2-DB');
	});

	it('names no upstream project in any failure message', () => {
		const reasons: ListEvent['reason'][] = ['no_dump', 'unreachable', 'bad_archive', 'failed'];
		for (const job of ['import_db', 'update'] as const) {
			for (const reason of reasons) {
				const { unmount } = setup({ ...failed(reason, 'details'), job });
				const alert = screen.getByRole('alert');
				const details = alert.querySelector('details');
				const outside = [...alert.childNodes].filter((n) => n !== details);
				expect(outside.map((n) => n.textContent).join('')).not.toContain('FMD2-DB');
				unmount();
			}
		}
	});

	it('says there is no ready-made list for the website and offers Update list', async () => {
		const { updateList } = setup(
			failed('no_dump', `${MODULE.id}: downloading ${URL} failed with HTTP status 404`)
		);

		const alert = screen.getByRole('alert');
		expect(alert.textContent).toContain(
			'There is no ready-made list for MangaDex yet. Use Update list to build it from the website.'
		);
		const details = alert.querySelector('details');
		expect(details?.textContent).toContain(URL);
		const outside = [...alert.childNodes].filter((n) => n !== details);
		expect(outside.map((n) => n.textContent).join('')).not.toContain(URL);

		await fireEvent.click(within(alert).getByRole('button', { name: 'Update list' }));
		expect(updateList).toHaveBeenCalledWith(MODULE.id);
	});

	it('does not point to Update list when the website cannot build its list', () => {
		setup(failed('no_dump', `${MODULE.id}: downloading ${URL} failed with HTTP status 404`), {
			...MODULE,
			capabilities: { ...MODULE.capabilities, update_list: false }
		});

		const alert = screen.getByRole('alert');
		expect(alert.textContent).toContain(
			'There is no ready-made list for MangaDex yet, and this website cannot build one itself.'
		);
		expect(within(alert).queryByRole('button', { name: 'Update list' })).toBeNull();
	});

	it('words unreachable ready-made lists plainly, details aside', () => {
		setup(failed('unreachable', `${MODULE.id}: downloading ${URL} failed with HTTP status 500`));

		const alert = screen.getByRole('alert');
		expect(alert.textContent).toContain(
			'Could not reach the ready-made lists to get the list of MangaDex.'
		);
		expect(alert.querySelector('details')?.textContent).toContain('HTTP status 500');
	});
});
