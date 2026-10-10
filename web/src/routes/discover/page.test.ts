// @vitest-environment jsdom
import { render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { api } from '#lib/app.ts';
import Discover from './+page.svelte';

/** Set to answer that this server has no MangaBaka database at all. */
const mangabaka = vi.hoisted(() => ({ unavailable: false, answered: false }));

// The page talks to the mock backend; no list job runs, so it needs no event store.
vi.mock('#lib/app.ts', async () => {
	const { createApi } = await import('#lib/api/client.ts');
	const { createMockBackend } = await import('#lib/api/mock.ts');
	const backend = createMockBackend().fetch;
	const fetch: typeof backend = async (input) => {
		const response = await backend(input);
		if (!mangabaka.unavailable || !input.url.endsWith('/api/metadata/mangabaka')) {
			return response;
		}
		mangabaka.answered = true;
		const status = { ...(await response.json()), available: false };
		return new Response(JSON.stringify(status), {
			headers: { 'content-type': 'application/json' }
		});
	};
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
	localStorage.clear();
	mangabaka.unavailable = false;
	mangabaka.answered = false;
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
		within(website).getByRole('combobox', { name: 'Status' });
		await waitFor(() => within(website).getByRole('group', { name: 'Genres' }));
	});

	it('says how to get the metadata filters while MangaBaka is not downloaded', async () => {
		localStorage.setItem('fmd2r.discover.mangabaka-hint-dismissed', '1');
		await api.patchSettings({ general: { selected_websites: ['webtoons'] } });
		render(Discover);

		const metadata = await screen.findByRole('group', { name: 'Metadata filters' });
		expect(screen.queryByRole('note')).toBeNull();
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
		render(Discover);

		await screen.findByRole('group', { name: 'Website filters' });
		await waitFor(() => expect(mangabaka.answered).toBe(true));
		await new Promise((resolve) => setTimeout(resolve, 0));
		expect(screen.queryByRole('group', { name: 'Metadata filters' })).toBeNull();
	});
});
