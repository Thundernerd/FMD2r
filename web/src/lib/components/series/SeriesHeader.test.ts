// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import type { Api } from '#lib/api/client.ts';
import type { FavoritePatch, FavoriteView, SeriesInfo } from '#lib/api/types.ts';
import SeriesHeader from './SeriesHeader.svelte';

/** A series with no facts beyond its title, changed by `overrides`. */
function seriesInfo(overrides: Partial<SeriesInfo>): SeriesInfo {
	return {
		module_id: 'm',
		link: '/s',
		url: 'https://example.com/s',
		title: 'Series',
		alt_titles: '',
		authors: '',
		artists: '',
		genres: [],
		summary: '',
		summary_from_mangabaka: false,
		status: 'ongoing',
		in_library: false,
		chapters: [],
		mangabaka_match: 'unavailable',
		...overrides
	};
}

describe('SeriesHeader', () => {
	let component: ReturnType<typeof mount> | null = null;

	afterEach(() => {
		if (component) unmount(component);
		component = null;
		document.body.innerHTML = '';
	});

	// Adding a series to the library marks its chapters too (frmMain.pas:2797-2846), so the count
	// is of chapters seen, not necessarily downloaded.
	it('counts the marked chapters as seen', () => {
		const series = seriesInfo({
			in_library: true,
			chapters: [
				{ name: 'Chapter 1', link: '/c/1', downloaded: true },
				{ name: 'Chapter 2', link: '/c/2', downloaded: false },
				{ name: 'Chapter 3', link: '/c/3', downloaded: true }
			]
		});
		// The header reads `api` from its buttons, which this test doesn't press, and for the group
		// select, which stays hidden when it cannot load.
		component = mount(SeriesHeader, {
			target: document.body,
			props: { api: {} as Api, series, website: 'Example' }
		});
		flushSync();

		const facts = [...document.querySelectorAll('.facts > div')].map((d) => [
			d.querySelector('dt')?.textContent,
			d.querySelector('dd')?.textContent
		]);
		expect(facts).toContainEqual(['Chapters', '3 · 2 seen']);
	});

	it("marks MangaBaka's description and shows its format and year", () => {
		const series = seriesInfo({
			summary: 'A description from MangaBaka.',
			summary_from_mangabaka: true,
			format: 'manhwa',
			year: 2022
		});
		component = mount(SeriesHeader, {
			target: document.body,
			props: { api: {} as Api, series, website: 'Example' }
		});
		flushSync();

		const summary = document.querySelector('.summary');
		expect(summary?.textContent).toContain('A description from MangaBaka.');
		expect(summary?.textContent).toContain('from MangaBaka');
		const facts = [...document.querySelectorAll('.facts > div')].map((d) => [
			d.querySelector('dt')?.textContent,
			d.querySelector('dd')?.textContent
		]);
		expect(facts).toContainEqual(['Format', 'Manhwa']);
		expect(facts).toContainEqual(['Year', '2022']);
	});

	it('offers a download folder only for a series in the library', () => {
		const folderButton = () =>
			[...document.querySelectorAll('button')].find(
				(b) => b.textContent?.trim() === 'Download folder…'
			);
		component = mount(SeriesHeader, {
			target: document.body,
			props: { api: {} as Api, series: seriesInfo({}), website: 'Example' }
		});
		flushSync();
		expect(folderButton()).toBeUndefined();

		unmount(component);
		component = mount(SeriesHeader, {
			target: document.body,
			props: { api: {} as Api, series: seriesInfo({ in_library: true }), website: 'Example' }
		});
		flushSync();
		expect(folderButton()).toBeDefined();
	});

	function linksOf(series: SeriesInfo) {
		component = mount(SeriesHeader, {
			target: document.body,
			props: { api: {} as Api, series, website: 'Example' }
		});
		flushSync();
		return shownLinks();
	}

	function shownLinks() {
		const row = [...document.querySelectorAll('.facts > div')].find(
			(d) => d.querySelector('dt')?.textContent === 'Links'
		);
		return [...(row?.querySelectorAll('a') ?? [])];
	}

	it('links to the series on its website, and only there without a MangaBaka match', () => {
		const links = linksOf(seriesInfo({}));

		expect(links.map((a) => [a.textContent, a.getAttribute('href')])).toEqual([
			['Example', 'https://example.com/s']
		]);
	});

	it("shows MangaBaka's links after the website's, each in a new tab", () => {
		const links = linksOf(
			seriesInfo({
				mangabaka: {
					id: 189,
					links: [
						{ label: 'MangaBaka', url: 'https://mangabaka.org/189' },
						{ label: 'AniList', url: 'https://anilist.co/manga/158766' }
					]
				}
			})
		);

		expect(links.map((a) => [a.textContent, a.getAttribute('href')])).toEqual([
			['Example', 'https://example.com/s'],
			['MangaBaka', 'https://mangabaka.org/189'],
			['AniList', 'https://anilist.co/manga/158766']
		]);
		for (const a of links) {
			expect(a.getAttribute('target')).toBe('_blank');
			expect(a.getAttribute('rel')).toBe('noreferrer');
		}
	});

	it('hides the links past eight behind "Show all"', () => {
		const many = Array.from({ length: 10 }, (_, i) => ({
			label: `site${i}.com`,
			url: `https://site${i}.com/s`
		}));
		const shown = linksOf(seriesInfo({ mangabaka: { id: 1, links: many } }));

		expect(shown).toHaveLength(8);
		const showAll = [...document.querySelectorAll('.facts button')].find(
			(b) => b.textContent === 'Show all'
		) as HTMLButtonElement;
		showAll.click();
		flushSync();
		expect(shownLinks()).toHaveLength(11);
	});

	/** The header's MangaBaka match control: its text and button names. */
	function matchControl(series: SeriesInfo) {
		component = mount(SeriesHeader, {
			target: document.body,
			props: { api: {} as Api, series, website: 'Example' }
		});
		flushSync();
		const control = document.querySelector('[aria-label="MangaBaka match"]');
		return {
			text: control?.textContent?.replace(/\s+/g, ' ').trim() ?? null,
			buttons: [...(control?.querySelectorAll('button') ?? [])].map((b) => b.textContent?.trim())
		};
	}

	const REBORN_RICH = {
		id: 189,
		links: [{ label: 'MangaBaka', url: 'https://mangabaka.org/189' }]
	};

	it('offers to match a series MangaBaka has no match for', () => {
		const control = matchControl(seriesInfo({ mangabaka_match: 'unmatched' }));

		expect(control.text).toContain('Not matched on MangaBaka');
		expect(control.buttons).toEqual(['Match…']);
	});

	it('offers to change an automatic match', () => {
		const control = matchControl(
			seriesInfo({ mangabaka_match: 'automatic', mangabaka: REBORN_RICH })
		);

		expect(control.buttons).toEqual(['Change…']);
		expect(control.text).not.toContain('by hand');
	});

	it('offers to change or undo a match made by hand', () => {
		const control = matchControl(seriesInfo({ mangabaka_match: 'manual', mangabaka: REBORN_RICH }));

		expect(control.text).toContain('matched by hand');
		expect(control.buttons).toEqual(['Change…', 'Undo']);
	});

	it('says when the series picked by hand left the database', () => {
		const control = matchControl(seriesInfo({ mangabaka_match: 'missing' }));

		expect(control.text).toContain(
			'The MangaBaka series picked by hand is no longer in the database'
		);
		expect(control.buttons).toEqual(['Match…']);
	});

	it('hides the match control without a MangaBaka database', () => {
		const control = matchControl(seriesInfo({ mangabaka_match: 'unavailable' }));

		expect(control.text).toBeNull();
		expect(document.body.textContent).not.toContain('Match…');
	});

	/** A library holding this series (favorite 7, in group `groupId`) and groups 1 and 2. */
	function groupedLibrary(groupId: number | null) {
		const favorite = { id: 7, module_id: 'm', link: '/s', group_id: groupId } as FavoriteView;
		const api = {
			listFavorites: vi.fn(() => Promise.resolve([favorite])),
			listGroups: vi.fn(() =>
				Promise.resolve([
					{ id: 1, name: 'Shounen', collapsed: false },
					{ id: 2, name: 'Seinen', collapsed: false }
				])
			),
			createGroup: vi.fn((name: string) => Promise.resolve({ id: 3, name, collapsed: false })),
			updateFavorite: vi.fn((id: number, patch: FavoritePatch) =>
				Promise.resolve({ ...favorite, ...patch })
			)
		};
		component = mount(SeriesHeader, {
			target: document.body,
			props: { api: api as unknown as Api, series: seriesInfo({ in_library: true }), website: 'E' }
		});
		return api;
	}

	async function groupSelect(): Promise<HTMLSelectElement> {
		await vi.waitFor(() => {
			if (!document.querySelector('select[aria-label="Group"] option[value="2"]')) {
				throw new Error('groups not loaded');
			}
		});
		return document.querySelector('select[aria-label="Group"]') as HTMLSelectElement;
	}

	function choose(select: HTMLSelectElement, value: string) {
		select.value = value;
		select.dispatchEvent(new Event('change', { bubbles: true }));
		flushSync();
	}

	it('says when the groups cannot load, and leaves the select out', async () => {
		const api = {
			listFavorites: async () => [{ id: 7, module_id: 'm', link: '/s', enabled: true }],
			listGroups: async () => {
				throw new Error('offline');
			}
		} as unknown as Api;
		component = mount(SeriesHeader, {
			target: document.body,
			props: { api, series: seriesInfo({ in_library: true }), website: 'E' }
		});
		await vi.waitFor(() =>
			expect(document.querySelector('[role="alert"]')?.textContent).toBe(
				'Could not load the groups.'
			)
		);
		expect(document.querySelector('select[aria-label="Group"]')).toBeNull();
	});

	it("shows the series' group and moves it to another or none", async () => {
		const api = groupedLibrary(1);
		const select = await groupSelect();
		expect([...select.options].map((o) => o.textContent)).toEqual([
			'No group',
			'Shounen',
			'Seinen',
			'New group…'
		]);
		expect(select.value).toBe('1');

		choose(select, '2');
		expect(api.updateFavorite).toHaveBeenLastCalledWith(7, { group_id: 2 });
		choose(select, '');
		expect(api.updateFavorite).toHaveBeenLastCalledWith(7, { group_id: null });
	});

	it('creates a group from the select and puts the series in it', async () => {
		const api = groupedLibrary(null);
		choose(await groupSelect(), 'new');

		const name = document.querySelector('input[aria-label="New group name"]') as HTMLInputElement;
		name.value = 'Isekai';
		name.dispatchEvent(new Event('input', { bubbles: true }));
		(name.form as HTMLFormElement).requestSubmit();
		await vi.waitFor(() => expect(api.updateFavorite).toHaveBeenCalledWith(7, { group_id: 3 }));
		expect(api.createGroup).toHaveBeenCalledWith('Isekai');
		await vi.waitFor(() =>
			expect(
				(document.querySelector('select[aria-label="Group"]') as HTMLSelectElement).value
			).toBe('3')
		);
	});

	it('keeps a group it created when moving the series into it fails', async () => {
		const api = groupedLibrary(null);
		api.updateFavorite.mockImplementationOnce(() => Promise.reject(new Error('offline')));
		choose(await groupSelect(), 'new');

		const name = document.querySelector('input[aria-label="New group name"]') as HTMLInputElement;
		name.value = 'Isekai';
		name.dispatchEvent(new Event('input', { bubbles: true }));
		(name.form as HTMLFormElement).requestSubmit();
		await vi.waitFor(() =>
			expect(document.querySelector('[role="alert"]')?.textContent).toBe(
				'Could not change the group.'
			)
		);
		const select = document.querySelector('select[aria-label="Group"]') as HTMLSelectElement;
		expect([...select.options].map((o) => o.textContent)).toContain('Isekai');
	});

	/** This series' library entry, `enabled` unless `overrides` say otherwise. */
	function favoriteView(overrides: Partial<FavoriteView>): FavoriteView {
		return {
			id: 7,
			module_id: 'm',
			website: 'Example',
			link: '/s',
			title: 'Series',
			display_title: 'Series',
			status: 'ongoing',
			enabled: true,
			save_to: '/data/Series',
			cover_url: '',
			current_chapter: 0,
			new_chapters: 0,
			date_added: '2026-01-01T00:00:00Z',
			...overrides
		};
	}

	/** A library with this series in it, whose updates are recorded in `patches`. */
	function libraryApi(entry: FavoriteView) {
		const patches: [number, FavoritePatch][] = [];
		const api = {
			listFavorites: async () => [entry],
			updateFavorite: async (id: number, patch: FavoritePatch) => {
				patches.push([id, patch]);
				entry = { ...entry, enabled: patch.enabled ?? entry.enabled };
				return entry;
			}
		} as unknown as Api;
		return { api, patches };
	}

	const button = (name: string) =>
		[...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === name);

	it('pauses checking a library series, then offers to resume it', async () => {
		const { api, patches } = libraryApi(favoriteView({ enabled: true }));
		component = mount(SeriesHeader, {
			target: document.body,
			props: { api, series: seriesInfo({ in_library: true }), website: 'Example' }
		});
		await vi.waitFor(() => expect(button('Pause checking')).toBeDefined());
		expect(document.body.textContent).not.toContain('Paused:');

		button('Pause checking')?.click();

		await vi.waitFor(() => expect(button('Resume checking')).toBeDefined());
		expect(patches).toEqual([[7, { enabled: false }]]);
		expect(document.body.textContent).toContain(
			'Paused: not checked for new chapters, so nothing new downloads.'
		);
	});

	it("can't check a paused series for missing chapters", async () => {
		const { api } = libraryApi(favoriteView({ enabled: false }));
		component = mount(SeriesHeader, {
			target: document.body,
			props: { api, series: seriesInfo({ in_library: true }), website: 'Example' }
		});
		await vi.waitFor(() => expect(button('Resume checking')).toBeDefined());

		const check = button('Check missing chapters');
		expect(check?.disabled).toBe(true);
		expect(check?.title).toBe('Resume checking to look for missing chapters');
	});

	it("can't check for missing chapters before knowing whether the series is paused", async () => {
		const api = {
			listFavorites: async () => {
				throw new Error('offline');
			}
		} as unknown as Api;
		component = mount(SeriesHeader, {
			target: document.body,
			props: { api, series: seriesInfo({ in_library: true }), website: 'Example' }
		});
		flushSync();
		expect(button('Check missing chapters')?.disabled).toBe(true);

		await vi.waitFor(() =>
			expect(document.querySelector('[role="alert"]')?.textContent).toBe(
				'Could not load this series from the library.'
			)
		);
		expect(button('Check missing chapters')?.disabled).toBe(true);
	});
});
