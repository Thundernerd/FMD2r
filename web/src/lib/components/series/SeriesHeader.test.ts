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
});
