// @vitest-environment jsdom
import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Api } from '#lib/api/client.ts';
import type { FavoriteView } from '#lib/api/types.ts';
import FailingNotice from './FailingNotice.svelte';

const favorite = (patch: Partial<FavoriteView>): FavoriteView => ({
	id: 7,
	module_id: 'webtoons',
	website: 'Webtoons',
	link: '/en/apothecary',
	title: 'The Apothecary Diaries',
	display_title: 'Kusuriya no Hitorigoto',
	status: 'ongoing',
	enabled: true,
	save_to: '/data',
	cover_url: '/api/covers/series',
	current_chapter: 76,
	new_chapters: 0,
	date_added: '2026-01-01T00:00:00Z',
	tags: [],
	check_failures: 0,
	...patch
});

/** The API with the library holding `favorites`, and a check that starts. */
function fakeApi(favorites: FavoriteView[]) {
	const checkFavorites = vi.fn<Api['checkFavorites']>(() => Promise.resolve());
	const api = {
		listFavorites: () => Promise.resolve(favorites),
		checkFavorites
	} as Pick<Api, 'listFavorites' | 'checkFavorites'> as Api;
	return { api, checkFavorites };
}

describe('FailingNotice', () => {
	it('says how many checks failed and why, and links to Discover searching for the title', async () => {
		const { api } = fakeApi([
			favorite({ check_failures: 4, last_check_error: 'HTTP 404 Not Found' })
		]);
		render(FailingNotice, { api, module: 'webtoons', link: '/en/apothecary' });

		const warning = await screen.findByRole('alert');
		expect(warning.textContent).toContain(
			"The website hasn't answered for this series in its last 4 checks: HTTP 404 Not Found"
		);
		const find = screen.getByRole('link', { name: 'Find on another website' });
		expect(find.getAttribute('href')).toBe('/discover?q=The+Apothecary+Diaries');
	});

	it('checks the series again on its own', async () => {
		const { api, checkFavorites } = fakeApi([
			favorite({ check_failures: 3, last_check_error: 'timed out' })
		]);
		render(FailingNotice, { api, module: 'webtoons', link: '/en/apothecary' });

		await fireEvent.click(await screen.findByRole('button', { name: 'Check again' }));

		expect(checkFavorites).toHaveBeenCalledWith([7]);
		await screen.findByText(/Checking again/);
	});

	it('shows nothing for a series with fewer than 3 failed checks, or not in the library', async () => {
		const listed = favorite({ check_failures: 2, last_check_error: 'timed out' });
		const { api } = fakeApi([listed]);
		render(FailingNotice, { api, module: 'webtoons', link: '/en/apothecary' });
		render(FailingNotice, { api, module: 'webtoons', link: '/elsewhere' });

		// Let the library load.
		await new Promise((resolve) => setTimeout(resolve, 0));
		expect(screen.queryByRole('alert')).toBeNull();
		expect(screen.queryByRole('link', { name: 'Find on another website' })).toBeNull();
	});
});
