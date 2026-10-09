// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import type { Api } from '#lib/api/client.ts';
import type { SeriesInfo } from '#lib/api/types.ts';
import DownloadBox from './DownloadBox.svelte';

const series: SeriesInfo = {
	module_id: 'm',
	link: '/s',
	title: 'Series',
	alt_titles: '',
	authors: '',
	artists: '',
	genres: [],
	summary: '',
	status: 'ongoing',
	in_library: true,
	chapters: [
		{ name: 'Chapter 1', link: '/c/1', downloaded: true },
		{ name: 'Chapter 2', link: '/c/2', downloaded: true },
		{ name: 'Chapter 3', link: '/c/3', downloaded: false }
	]
};

describe('DownloadBox', () => {
	let component: ReturnType<typeof mount> | null = null;

	afterEach(() => {
		if (component) unmount(component);
		component = null;
		document.body.innerHTML = '';
	});

	// Adding a series to the library marks its chapters too (frmMain.pas:2797-2846), so a marked
	// chapter was seen, not necessarily downloaded.
	it('counts the picked marked chapters as seen before', () => {
		// The box only reads `api` when queuing, which this test doesn't do.
		component = mount(DownloadBox, {
			target: document.body,
			props: {
				api: {} as Api,
				series,
				selected: new Set([0, 1, 2]),
				saveTo: '',
				format: 'cbz',
				onqueued: () => {}
			}
		});
		flushSync();

		expect(document.querySelector('.summary')?.textContent?.replace(/\s+/g, ' ').trim()).toBe(
			'3 chapters selected · 2 seen before'
		);
	});
});
