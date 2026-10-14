// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { api } from '#lib/app.ts';
import Library from './+page.svelte';

vi.mock('$app/state', () => import('#lib/testing/app.fake.svelte.ts'));
vi.mock('$app/navigation', () => import('#lib/testing/app.fake.svelte.ts'));

// The page talks to the mock backend; no favorites check runs, so the event store stays empty.
vi.mock('#lib/app.ts', async () => {
	const { createApi } = await import('#lib/api/client.ts');
	const { createMockBackend } = await import('#lib/api/mock.ts');
	const { fetch } = createMockBackend();
	return {
		api: createApi({ baseUrl: 'http://fmd2r.test', fetch }),
		events: { jobs: {}, favorites: null, seed: () => {} }
	};
});

afterEach(async () => {
	await api.patchSettings({ appearance: { library_title: 'website' } });
});

/** The titles on the cards, in order. */
async function cardTitles(): Promise<string[]> {
	const grid = await screen.findByRole('list', { name: 'Favorites' });
	return within(grid)
		.getAllByRole('link')
		.map((card) => card.querySelector('.t')?.textContent ?? '');
}

describe('Library titles', () => {
	it('shows the website’s titles by default, without a tooltip', async () => {
		render(Library);

		const titles = await cardTitles();
		expect(titles).toContain('Frieren');
		const frieren = screen.getByText('Frieren', { selector: '.t' });
		expect(frieren.getAttribute('title')).toBeNull();
	});

	it('shows the chosen title, with the website’s as a tooltip', async () => {
		await api.patchSettings({ appearance: { library_title: 'native' } });
		render(Library);

		await waitFor(async () => expect(await cardTitles()).toContain('葬送のフリーレン'));
		const frieren = screen.getByText('葬送のフリーレン', { selector: '.t' });
		expect(frieren.getAttribute('title')).toBe('Frieren');
		// A series MangaBaka does not know keeps the website's title, and needs no tooltip.
		const sakamoto = screen.getByText('Sakamoto Days', { selector: '.t' });
		expect(sakamoto.getAttribute('title')).toBeNull();
	});

	it('sorts and searches by the shown title, and still finds the website’s', async () => {
		await api.patchSettings({ appearance: { library_title: 'native' } });
		render(Library);
		await waitFor(async () => expect(await cardTitles()).toContain('葬送のフリーレン'));

		await fireEvent.change(screen.getByRole('combobox', { name: 'Sort' }), {
			target: { value: 'title' }
		});
		await waitFor(async () => expect((await cardTitles()).at(-1)).toBe('葬送のフリーレン'));

		const search = screen.getByRole('searchbox', { name: 'Search the library' });
		await fireEvent.input(search, { target: { value: 'フリーレン' } });
		await waitFor(async () => expect(await cardTitles()).toEqual(['葬送のフリーレン']));
		await fireEvent.input(search, { target: { value: 'frieren' } });
		await waitFor(async () => expect(await cardTitles()).toEqual(['葬送のフリーレン']));
	});
});

describe('Library selection', () => {
	it('selects cards instead of opening them, and pauses the selection in one request', async () => {
		const bulk = vi.spyOn(api, 'bulkUpdateFavorites');
		render(Library);
		const grid = await screen.findByRole('list', { name: 'Favorites' });
		expect(within(grid).getAllByRole('link').length).toBeGreaterThan(0);

		await fireEvent.click(screen.getByRole('button', { name: 'Select' }));

		expect(within(grid).queryAllByRole('link')).toEqual([]);
		const frieren = within(grid).getByRole('checkbox', { name: 'Frieren' });
		const sakamoto = within(grid).getByRole('checkbox', { name: 'Sakamoto Days' });
		await fireEvent.click(frieren);
		await fireEvent.click(sakamoto);
		expect(frieren.getAttribute('aria-checked')).toBe('true');
		expect(screen.getByRole('status', { name: 'Selection' }).textContent).toContain('2 selected');

		await fireEvent.click(screen.getByRole('button', { name: 'Pause' }));

		await waitFor(() => expect(bulk).toHaveBeenCalledTimes(1));
		expect(bulk).toHaveBeenCalledWith({ ids: [1, 4], enabled: false });
		await waitFor(() =>
			expect(within(grid).getByRole('checkbox', { name: 'Frieren' }).textContent).toContain(
				'Paused'
			)
		);
		// The selection stays for the next action.
		expect(
			within(grid).getByRole('checkbox', { name: 'Frieren' }).getAttribute('aria-checked')
		).toBe('true');

		await api.bulkUpdateFavorites({ ids: [1, 4], enabled: true });
		bulk.mockRestore();
	});
});

describe('Library failing series', () => {
	it('marks a series whose website keeps failing, and lists it under Failing', async () => {
		render(Library);
		const grid = await screen.findByRole('list', { name: 'Favorites' });
		const card = within(grid).getByRole('link', { name: /The Apothecary Diaries/ });
		expect(within(card).getByText('Not found')).toBeTruthy();
		const frieren = within(grid).getByRole('link', { name: /Frieren/ });
		expect(within(frieren).queryByText('Not found')).toBeNull();

		await fireEvent.click(screen.getByRole('button', { name: /^Failing/ }));
		await waitFor(async () => expect(await cardTitles()).toEqual(['The Apothecary Diaries']));
	});
});

describe('Library chapter counts', () => {
	it('shows each checked series’ chapter count on its cover, beside its new chapters', async () => {
		render(Library);
		const grid = await screen.findByRole('list', { name: 'Favorites' });
		const card = within(grid).getByRole('link', { name: /Frieren/ });
		const count = within(card).getByLabelText('142 chapters');
		expect(count.textContent).toBe('142');
		expect(count.getAttribute('title')).toBe('142 chapters');
		expect(within(card).getByLabelText('4 new chapters').textContent).toBe('+4');
	});

	it('shows no count for a series not checked yet', async () => {
		const list = api.listFavorites.bind(api);
		const spy = vi
			.spyOn(api, 'listFavorites')
			.mockImplementation(async (...args) =>
				(await list(...args)).map((favorite) =>
					favorite.title === 'Sakamoto Days' ? { ...favorite, current_chapter: 0 } : favorite
				)
			);
		try {
			render(Library);
			const grid = await screen.findByRole('list', { name: 'Favorites' });
			const card = within(grid).getByRole('link', { name: /Sakamoto Days/ });
			expect(within(card).queryByLabelText(/chapters$/)).toBeNull();
		} finally {
			spy.mockRestore();
		}
	});
});
