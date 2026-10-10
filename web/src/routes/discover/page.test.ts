// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { api } from '#lib/app.ts';
import Discover from './+page.svelte';

/** Set to answer that this server has no MangaBaka database at all. */
const mangabaka = vi.hoisted(() => ({ unavailable: false }));

// The page talks to the mock backend; no list job runs, so it needs no event store.
vi.mock('#lib/app.ts', async () => {
	const { createApi } = await import('#lib/api/client.ts');
	const { createMockBackend } = await import('#lib/api/mock.ts');
	const backend = createMockBackend().fetch;
	const unavailable = createMockBackend({ mangabakaAvailable: false }).fetch;
	const fetch = (input: Request) => (mangabaka.unavailable ? unavailable : backend)(input);
	return { api: createApi({ baseUrl: 'http://fmd2r.test', fetch }), events: {} };
});

/** Every observed element is on screen at once. */
class OnScreen {
	constructor(private readonly callback: IntersectionObserverCallback) {}
	observe(target: Element) {
		const entry = { isIntersecting: true, target } as unknown as IntersectionObserverEntry;
		this.callback([entry], this as unknown as IntersectionObserver);
	}
	disconnect() {}
	unobserve() {}
	takeRecords() {
		return [];
	}
}

afterEach(() => {
	vi.unstubAllGlobals();
	vi.restoreAllMocks();
	mangabaka.unavailable = false;
});

describe('Discover', () => {
	it('asks for the covers of the titles on screen', async () => {
		vi.stubGlobal('IntersectionObserver', OnScreen);
		await api.patchSettings({
			general: { selected_websites: ['webtoons'], load_covers: true }
		});
		const { container } = render(Discover);

		await waitFor(() => expect(container.querySelectorAll('.card').length).toBeGreaterThan(0));
		const img = container.querySelector('.card img');
		expect(img?.getAttribute('src')).toMatch(/^\/api\/covers\/series\?module=.*&w=300$/);
	});

	it('asks for no cover with “Load manga covers” off', async () => {
		vi.stubGlobal('IntersectionObserver', OnScreen);
		await api.patchSettings({
			general: { selected_websites: ['webtoons'], load_covers: false }
		});
		const { container } = render(Discover);

		await waitFor(() => expect(container.querySelectorAll('.card').length).toBeGreaterThan(0));
		expect(container.querySelector('img')).toBeNull();
		expect(container.querySelector('.card .cover.blank')).not.toBeNull();
	});

	it('links to the website selection when no website is selected', async () => {
		await api.patchSettings({ general: { selected_websites: [] } });
		render(Discover);

		const empty = await screen.findByText(/No websites are selected/);
		const link = screen.getByRole('link', { name: 'Choose websites' });
		expect(empty.contains(link)).toBe(true);
		expect(link.getAttribute('href')).toBe('/settings#section-websites');
		expect(screen.queryByRole('combobox', { name: 'Website' })).toBeNull();
	});

	it('works as before without the MangaBaka database, pointing to it once', async () => {
		await api.patchSettings({ general: { selected_websites: ['webtoons'] } });
		render(Discover);

		await screen.findByRole('combobox', { name: 'Status' });
		expect(await screen.findByRole('note')).toBeTruthy();
		expect(screen.queryByRole('combobox', { name: 'Format' })).toBeNull();
		expect(screen.queryByRole('combobox', { name: 'Publication' })).toBeNull();
	});

	it('groups Status and the genres as website filters', async () => {
		await api.patchSettings({ general: { selected_websites: ['webtoons'] } });
		render(Discover);

		const website = await screen.findByRole('group', { name: 'Website filters' });
		expect(within(website).getByText('From each website’s list.')).toBeTruthy();
		expect(within(website).getByRole('combobox', { name: 'Status' })).toBeTruthy();
		expect(await within(website).findByRole('group', { name: 'Genres' })).toBeTruthy();
	});

	it('says how to get the metadata filters while MangaBaka is not downloaded', async () => {
		await api.patchSettings({ general: { selected_websites: ['webtoons'] } });
		render(Discover);

		const hint = await screen.findByRole('note');
		await fireEvent.click(within(hint).getByRole('button', { name: 'Dismiss' }));
		expect(screen.queryByRole('note')).toBeNull();
		const metadata = screen.getByRole('group', { name: 'Metadata filters' });
		expect(metadata.textContent?.replace(/\s+/g, ' ')).toContain(
			'Download the MangaBaka database to filter by format and publication.'
		);
		const link = within(metadata).getByRole('link');
		expect(link.getAttribute('href')).toBe('/settings#section-metadata');
		expect(within(metadata).queryByRole('combobox')).toBeNull();
	});

	it('leaves out the metadata filters when MangaBaka is unavailable', async () => {
		mangabaka.unavailable = true;
		await api.patchSettings({ general: { selected_websites: ['webtoons'] } });
		const asked = vi.spyOn(api, 'mangabakaStatus');
		render(Discover);

		await screen.findByRole('group', { name: 'Website filters' });
		await waitFor(() => expect(asked).toHaveBeenCalled());
		expect((await asked.mock.results[0]?.value)?.available).toBe(false);
		await tick();
		expect(screen.queryByRole('group', { name: 'Metadata filters' })).toBeNull();
	});
});
