// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import type { Api } from '#lib/api/client.ts';
import type { SeriesInfo } from '#lib/api/types.ts';
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
		// The header only reads `api` from its buttons, which this test doesn't press.
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
});
