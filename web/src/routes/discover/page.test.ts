// @vitest-environment jsdom
import { render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { api } from '#lib/app.ts';
import Discover from './+page.svelte';

// The page talks to the mock backend; no list job runs, so it needs no event store.
vi.mock('#lib/app.ts', async () => {
	const { createApi } = await import('#lib/api/client.ts');
	const { createMockBackend } = await import('#lib/api/mock.ts');
	const fetch = createMockBackend().fetch;
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

afterEach(() => vi.unstubAllGlobals());

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
});
