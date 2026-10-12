// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import type { Api } from '#lib/api/client.ts';
import type { SeriesInfo } from '#lib/api/types.ts';
import SeriesHeader from './SeriesHeader.svelte';

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
		const series: SeriesInfo = {
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
			in_library: true,
			chapters: [
				{ name: 'Chapter 1', link: '/c/1', downloaded: true },
				{ name: 'Chapter 2', link: '/c/2', downloaded: false },
				{ name: 'Chapter 3', link: '/c/3', downloaded: true }
			]
		};
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
		const series: SeriesInfo = {
			module_id: 'm',
			link: '/s',
			url: 'https://example.com/s',
			title: 'Series',
			alt_titles: '',
			authors: '',
			artists: '',
			genres: [],
			summary: 'A description from MangaBaka.',
			summary_from_mangabaka: true,
			format: 'manhwa',
			year: 2022,
			status: 'ongoing',
			in_library: false,
			chapters: []
		};
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

	const unmatched: SeriesInfo = {
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
		mangabaka: null
	};

	it('links to the series on its website, and only there without a MangaBaka match', () => {
		const links = linksOf(unmatched);

		expect(links.map((a) => [a.textContent, a.getAttribute('href')])).toEqual([
			['Example', 'https://example.com/s']
		]);
	});

	it("shows MangaBaka's links after the website's, each in a new tab", () => {
		const links = linksOf({
			...unmatched,
			mangabaka: {
				id: 189,
				links: [
					{ label: 'MangaBaka', url: 'https://mangabaka.org/189' },
					{ label: 'AniList', url: 'https://anilist.co/manga/158766' }
				]
			}
		});

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
		const shown = linksOf({ ...unmatched, mangabaka: { id: 1, links: many } });

		expect(shown).toHaveLength(8);
		const showAll = [...document.querySelectorAll('.facts button')].find(
			(b) => b.textContent === 'Show all'
		) as HTMLButtonElement;
		showAll.click();
		flushSync();
		expect(shownLinks()).toHaveLength(11);
	});
});
