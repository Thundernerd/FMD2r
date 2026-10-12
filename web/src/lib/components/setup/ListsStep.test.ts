// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError, createApi, type Api } from '#lib/api/client.ts';
import { createMockBackend } from '#lib/api/mock.ts';
import { EventStore } from '#lib/events.svelte.ts';
import type { ListEvent } from '#lib/api/types.ts';
import type { SetupStep } from '#lib/setup/steps.ts';
import FakeStep from './FakeStep.fixture.svelte';
import ListsStep from './ListsStep.svelte';
import SetupWizard from './SetupWizard.svelte';

const STEPS: SetupStep[] = [
	{ id: 'lists', title: 'Ready-made lists', component: ListsStep },
	{ id: 'after', title: 'After', component: FakeStep }
];

/** An event of the ready-made list download of `module`. */
const event = (module: string, patch: Partial<ListEvent>): ListEvent => ({
	module_id: module,
	job: 'import_db',
	kind: 'progress',
	status_text: '',
	done: 0,
	total: 0,
	titles: null,
	error: null,
	reason: null,
	...patch
});

/**
 * The wizard on the lists step of a fresh install that selected MangaDex, which has a list, and
 * Bato.to, which has none; its API calls spied on.
 */
async function open(override: Partial<Api> = {}, events: ListEvent[] = []) {
	const real = createApi({
		baseUrl: 'http://fmd2r.test',
		fetch: createMockBackend({ setUp: false }).fetch
	});
	await real.patchSettings({ general: { selected_websites: ['mangadex', 'batoto'] } });
	const api: Api = { ...real, ...override };
	const importListDb = vi.spyOn(api, 'importListDb');
	const store = new EventStore({ url: '/api/events', connect: () => ({}) as never });
	for (const e of events) store.lists[e.module_id] = e;
	render(SetupWizard, { api, store, steps: STEPS, onfinish: vi.fn() });
	await screen.findByRole('checkbox', { name: 'Bato.to' });
	return { api, store, importListDb };
}

const button = (name: string) => screen.getByRole('button', { name }) as HTMLButtonElement;
const checkbox = (name: string) => screen.getByRole('checkbox', { name }) as HTMLInputElement;

describe('the ready-made lists setup step', () => {
	afterEach(() => {
		cleanup();
		sessionStorage.clear();
	});

	it('checks the selected websites without a list, and not the ones with one', async () => {
		await open();
		expect(checkbox('Bato.to').checked).toBe(true);
		expect(checkbox('MangaDex').checked).toBe(false);
		expect(screen.queryByRole('checkbox', { name: 'Webtoons' })).toBeNull();

		const mangadex = screen.getByRole('group', { name: 'MangaDex' });
		expect(mangadex.textContent).toMatch(/140 titles · updated/);
		expect(document.body.textContent).toMatch(/Discover needs one\s+per website/);
		expect(document.body.textContent).toContain('You can get or update lists later in Discover.');
	});

	it("downloads the checked websites' lists, reading Skip until it starts", async () => {
		const { api, importListDb } = await open();
		expect(button('Skip').disabled).toBe(false);
		await fireEvent.click(checkbox('MangaDex'));

		await fireEvent.click(button('Download selected'));
		expect(importListDb).toHaveBeenCalledTimes(2);
		expect(importListDb).toHaveBeenCalledWith('batoto');
		expect(importListDb).toHaveBeenCalledWith('mangadex');
		// Started, so another click doesn't start them again.
		expect(button('Download selected').disabled).toBe(true);
		await vi.waitFor(() => expect(button('Next').disabled).toBe(false));

		await fireEvent.click(button('Next'));
		await screen.findByRole('heading', { level: 2, name: 'After' });
		const running = (await api.listModules()).filter((m) => m.list_job_running);
		expect(running.map((m) => m.id).sort()).toEqual(['batoto', 'mangadex']);
	});

	it('follows each download: its progress, its titles, or why it failed', async () => {
		const { store } = await open();
		await fireEvent.click(checkbox('MangaDex'));
		await fireEvent.click(button('Download selected'));
		const batoto = screen.getByRole('group', { name: 'Bato.to' });
		const mangadex = screen.getByRole('group', { name: 'MangaDex' });

		store.lists['batoto'] = event('batoto', { done: 1, total: 4 });
		store.lists['mangadex'] = event('mangadex', {
			kind: 'finished',
			done: 3,
			total: 3,
			titles: 60
		});
		const bar = await within(batoto).findByRole('progressbar');
		expect(bar.getAttribute('aria-valuenow')).toBe('1');
		expect(bar.getAttribute('aria-valuemax')).toBe('4');
		await vi.waitFor(() => expect(mangadex.textContent).toContain('Imported 60 titles.'));

		store.lists['batoto'] = event('batoto', { kind: 'failed', reason: 'no_dump' });
		const alert = await within(batoto).findByRole('alert');
		expect(alert.textContent).toContain('There is no ready-made list for Bato.to yet.');
		expect(alert.textContent).toContain('Update list in Discover builds one');
		expect(within(batoto).queryByRole('progressbar')).toBeNull();
	});

	it('hides the download when the server has no list jobs, pointing to Discover', async () => {
		const unavailable = () =>
			Promise.reject(new ApiError(503, 'importListDb', 'list jobs are not available'));
		const { importListDb } = await open({ importListDb: unavailable });

		await fireEvent.click(button('Download selected'));
		expect(importListDb).toHaveBeenCalledOnce();
		expect((await screen.findByRole('alert')).textContent).toMatch(/Discover can get them later/);
		expect(screen.queryByRole('button', { name: 'Download selected' })).toBeNull();
		expect(button('Skip').disabled).toBe(false);
	});

	it('reads Next when a download runs already, as when coming back to the step', async () => {
		await open({}, [event('batoto', { done: 1, total: 4 })]);
		expect(screen.queryByRole('button', { name: 'Skip' })).toBeNull();
		expect(button('Next').disabled).toBe(false);
	});

	it("shows a new download as running, not the last one's failure", async () => {
		await open({}, [event('batoto', { kind: 'failed', reason: 'unreachable' })]);
		const batoto = screen.getByRole('group', { name: 'Bato.to' });
		expect(within(batoto).getByRole('alert').textContent).toMatch(/Could not reach/);

		await fireEvent.click(button('Download selected'));
		await vi.waitFor(() =>
			expect(within(batoto).getByRole('status').textContent).toContain(
				'Getting the ready-made list'
			)
		);
		expect(within(batoto).queryByRole('alert')).toBeNull();
	});
});
