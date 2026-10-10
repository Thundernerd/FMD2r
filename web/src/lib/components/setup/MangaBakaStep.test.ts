// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError, createApi, type Api } from '#lib/api/client.ts';
import { createMockBackend } from '#lib/api/mock.ts';
import type { MetadataEvent } from '#lib/api/types.ts';
import { EventStore } from '#lib/events.svelte.ts';
import type { SetupStep } from '#lib/setup/steps.ts';
import FakeStep from './FakeStep.fixture.svelte';
import MangaBakaStep from './MangaBakaStep.svelte';
import SetupWizard from './SetupWizard.svelte';

const STEPS: SetupStep[] = [
	{ id: 'mangabaka', title: 'Metadata', component: MangaBakaStep },
	{ id: 'after', title: 'After', component: FakeStep }
];

const PROGRESS: MetadataEvent = {
	kind: 'progress',
	phase: 'downloading',
	status_text: 'Downloading and building...',
	done: 97_000_000,
	total: 388_000_000,
	error: null
};

/** The wizard on the MangaBaka step of a fresh install, its API calls spied on. */
async function open(override: Partial<Api> = {}) {
	const real = createApi({
		baseUrl: 'http://fmd2r.test',
		fetch: createMockBackend({ setUp: false }).fetch
	});
	const api: Api = { ...real, ...override };
	const download = vi.spyOn(api, 'downloadMangabaka');
	const store = new EventStore({ url: '/api/events', connect: () => ({}) as never });
	render(SetupWizard, { api, store, steps: STEPS, onfinish: vi.fn() });
	await screen.findByRole('heading', { level: 2, name: 'Metadata' });
	return { api, store, download };
}

const button = (name: string) => screen.getByRole('button', { name }) as HTMLButtonElement;

describe('the MangaBaka setup step', () => {
	afterEach(() => {
		cleanup();
		sessionStorage.clear();
	});

	it('explains the database, and downloads it in the background when chosen', async () => {
		const { api, store, download } = await open();
		const text = document.body.textContent ?? '';
		expect(text).toContain('390 MB');
		expect(text).toMatch(/covers/i);
		expect(text).toMatch(/Settings/);

		await fireEvent.click(await screen.findByRole('button', { name: 'Download now' }));
		expect(download).toHaveBeenCalledOnce();

		store.metadata = PROGRESS;
		const bar = await screen.findByRole('progressbar', { name: 'Download progress' });
		await vi.waitFor(() => expect(bar.getAttribute('aria-valuenow')).toBe('25'));

		expect(button('Next').disabled).toBe(false);
		await fireEvent.click(button('Next'));
		await screen.findByRole('heading', { level: 2, name: 'After' });
		expect((await api.mangabakaStatus()).running).toBe(true);
	});

	it('asks for a choice, and starts nothing when skipped', async () => {
		const { api, download } = await open();
		await screen.findByRole('button', { name: 'Download now' });
		expect(button('Next').disabled).toBe(true);

		await fireEvent.click(button('Skip'));
		expect(button('Skip').getAttribute('aria-pressed')).toBe('true');
		expect(button('Next').disabled).toBe(false);
		await fireEvent.click(button('Next'));
		await screen.findByRole('heading', { level: 2, name: 'After' });
		expect(download).not.toHaveBeenCalled();
		expect((await api.mangabakaStatus()).running).toBe(false);
	});

	it("says why when the server can't download it, and only offers Next", async () => {
		const reason = 'the MangaBaka database is not available';
		const { download } = await open({
			downloadMangabaka: () => Promise.reject(new ApiError(503, 'downloadMangabaka', reason))
		});

		await fireEvent.click(await screen.findByRole('button', { name: 'Download now' }));
		expect(download).toHaveBeenCalledOnce();
		expect((await screen.findByRole('alert')).textContent).toContain(reason);
		expect(screen.queryByRole('button', { name: 'Download now' })).toBeNull();
		expect(screen.queryByRole('button', { name: 'Skip' })).toBeNull();
		expect(button('Next').disabled).toBe(false);
	});

	it("says so up front when the server's status can't download it", async () => {
		const real = createApi({ baseUrl: 'http://fmd2r.test', fetch: createMockBackend().fetch });
		const status = await real.mangabakaStatus();
		await open({ mangabakaStatus: () => Promise.resolve({ ...status, available: false }) });

		expect((await screen.findByRole('alert')).textContent).toMatch(/can’t download/);
		expect(screen.queryByRole('button', { name: 'Download now' })).toBeNull();
		expect(screen.queryByRole('button', { name: 'Skip' })).toBeNull();
		expect(button('Next').disabled).toBe(false);
	});

	it('can still be skipped when the status fails to load', async () => {
		await open({ mangabakaStatus: () => Promise.reject(new Error('offline')) });

		await screen.findByRole('alert');
		await fireEvent.click(button('Skip'));
		expect(button('Next').disabled).toBe(false);
	});
});
