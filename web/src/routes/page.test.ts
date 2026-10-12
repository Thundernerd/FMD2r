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
