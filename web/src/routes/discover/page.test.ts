// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { SvelteURL } from 'svelte/reactivity';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { goto } from '$app/navigation';
import { api } from '#lib/app.ts';
import { page } from '#lib/testing/app.fake.svelte.ts';
import Discover from './+page.svelte';

vi.mock('$app/state', () => import('#lib/testing/app.fake.svelte.ts'));
vi.mock('$app/navigation', async () => {
	const fake = await import('#lib/testing/app.fake.svelte.ts');
	return { ...fake, goto: vi.fn(fake.goto) };
});

// The page talks to the mock backend; no list job runs, so the event store stays empty.
vi.mock('#lib/app.ts', async () => {
	const { createApi } = await import('#lib/api/client.ts');
	const { createMockBackend } = await import('#lib/api/mock.ts');
	const fetch = createMockBackend().fetch;
	return { api: createApi({ baseUrl: 'http://fmd2r.test', fetch }), events: { lists: {} } };
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
	vi.mocked(goto).mockClear();
	page.url = new SvelteURL('http://fmd2r.test/discover');
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

	it('opens with the search its URL names, with those filters selected', async () => {
		vi.stubGlobal('IntersectionObserver', OnScreen);
		await api.patchSettings({ general: { selected_websites: ['mangadex'] } });
		const search = vi.spyOn(api, 'searchLists');
		page.url = new SvelteURL(
			'http://fmd2r.test/discover?module=mangadex&q=the&genres_include=Action&status=1'
		);
		render(Discover);

		await waitFor(() => expect(search).toHaveBeenCalled());
		expect(search.mock.calls[0]?.[0]).toEqual({
			module: 'mangadex',
			q: 'the',
			genres_include: 'Action',
			status: '1'
		});
		expect(screen.getByRole<HTMLInputElement>('searchbox', { name: 'Search titles' }).value).toBe(
			'the'
		);
		expect(screen.getByRole<HTMLSelectElement>('combobox', { name: 'Status' }).value).toBe('1');
		expect(await screen.findByRole('button', { name: 'Action: included' })).toBeTruthy();
		await waitFor(() =>
			expect(screen.getByRole<HTMLSelectElement>('combobox', { name: 'Website' }).value).toBe(
				'mangadex'
			)
		);
		expect(goto).not.toHaveBeenCalled();
	});

	it('replaces its URL when a filter changes', async () => {
		vi.stubGlobal('IntersectionObserver', OnScreen);
		await api.patchSettings({ general: { selected_websites: ['mangadex'] } });
		page.url = new SvelteURL('http://fmd2r.test/discover?module=mangadex');
		render(Discover);

		const status = await screen.findByRole<HTMLSelectElement>('combobox', { name: 'Status' });
		await fireEvent.change(status, { target: { value: '0' } });

		await waitFor(() =>
			expect(goto).toHaveBeenCalledWith('/discover?module=mangadex&status=0', {
				replaceState: true,
				reset: false
			})
		);
	});
});
