// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Api } from '#lib/api/client.ts';
import type { MangaBakaStatus, MetadataEvent } from '#lib/api/types.ts';
import { EventStore } from '#lib/events.svelte.ts';
import MangaBakaPanel from './MangaBakaPanel.svelte';

const NONE: MangaBakaStatus = {
	available: true,
	downloaded: false,
	built_at: null,
	bytes: null,
	running: false,
	progress: null,
	next_refresh: null
};

const DOWNLOADED: MangaBakaStatus = {
	...NONE,
	downloaded: true,
	built_at: '2026-10-08T22:42:02Z',
	bytes: 214_000_000,
	next_refresh: '2026-10-15T22:42:02Z'
};

function setup(statuses: MangaBakaStatus[]) {
	const store = new EventStore({ url: '/api/events', connect: () => ({}) as never });
	let call = 0;
	const mangabakaStatus = vi.fn(() =>
		Promise.resolve(statuses[Math.min(call++, statuses.length - 1)] as MangaBakaStatus)
	);
	const downloadMangabaka = vi.fn(() => Promise.resolve());
	const removeMangabaka = vi.fn(() => Promise.resolve());
	const cancelMangabaka = vi.fn(() => Promise.resolve());
	const api = {
		mangabakaStatus,
		downloadMangabaka,
		removeMangabaka,
		cancelMangabaka
	} as unknown as Api;
	render(MangaBakaPanel, { api, store });
	return { store, downloadMangabaka, removeMangabaka, cancelMangabaka };
}

describe('MangaBakaPanel', () => {
	it('downloads only when asked, saying how large the download is', async () => {
		const { downloadMangabaka } = setup([NONE, { ...NONE, running: true }]);

		expect(await screen.findByText(/Not downloaded/)).toBeTruthy();
		expect(screen.getByText(/390 MB/)).toBeTruthy();
		expect(screen.queryByRole('button', { name: 'Remove' })).toBeNull();
		expect(downloadMangabaka).not.toHaveBeenCalled();

		await fireEvent.click(screen.getByRole('button', { name: 'Download' }));
		expect(downloadMangabaka).toHaveBeenCalledOnce();
		expect(await screen.findByRole('button', { name: 'Cancel' })).toBeTruthy();
	});

	it("shows the database's date and size, and updates it", async () => {
		const { downloadMangabaka } = setup([DOWNLOADED]);

		const date = await screen.findByText(/Built/);
		expect(date.textContent).toContain('2026');
		expect(screen.getByText(/214 MB/)).toBeTruthy();
		await fireEvent.click(screen.getByRole('button', { name: 'Update' }));
		expect(downloadMangabaka).toHaveBeenCalledOnce();
	});

	it('removes the database', async () => {
		const { removeMangabaka } = setup([DOWNLOADED, NONE]);

		await fireEvent.click(await screen.findByRole('button', { name: 'Remove' }));
		expect(removeMangabaka).toHaveBeenCalledOnce();
		expect(await screen.findByText(/Not downloaded/)).toBeTruthy();
	});

	it('shows the progress of a running download', async () => {
		const { store } = setup([{ ...NONE, running: true }]);
		await screen.findByRole('button', { name: 'Cancel' });

		const event: MetadataEvent = {
			kind: 'progress',
			phase: 'downloading',
			status_text: 'Downloading and building... 1000 series',
			done: 97_000_000,
			total: 388_000_000,
			error: null
		};
		store.metadata = event;

		const bar = await screen.findByRole('progressbar');
		expect(bar.getAttribute('aria-valuenow')).toBe('25');
		expect(screen.getByText(/1000 series/)).toBeTruthy();
	});

	it('refreshes its status when a download ends', async () => {
		const s = setup([{ ...NONE, running: true }, DOWNLOADED]);
		await screen.findByRole('button', { name: 'Cancel' });

		s.store.metadata = {
			kind: 'finished',
			phase: 'matching',
			status_text: '',
			done: 0,
			total: 0,
			error: null
		};

		expect(await screen.findByText(/214 MB/)).toBeTruthy();
	});

	it('shows why a download failed', async () => {
		const s = setup([{ ...NONE, running: true }, NONE]);
		await screen.findByRole('button', { name: 'Cancel' });

		s.store.metadata = {
			kind: 'failed',
			phase: 'downloading',
			status_text: '',
			done: 0,
			total: 0,
			error: "MangaBaka's database format changed: series 1 (line 1) has no `title` field"
		};

		const alert = await screen.findByRole('alert');
		expect(alert.textContent).toContain('format changed');
		await waitFor(() => expect(screen.getByRole('button', { name: 'Download' })).toBeTruthy());
	});
});
