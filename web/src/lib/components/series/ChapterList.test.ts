// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import ChapterList from './ChapterList.svelte';

// jsdom has no ResizeObserver; the list's `bind:clientHeight` needs one.
globalThis.ResizeObserver ??= class {
	observe() {}
	unobserve() {}
	disconnect() {}
};

/** The "Hide seen" checkbox. */
function hideSeen(): HTMLInputElement {
	const label = [...document.querySelectorAll('label')].find(
		(l) => l.textContent?.trim() === 'Hide seen'
	);
	const input = label?.querySelector('input');
	if (!input) throw new Error('no Hide seen toggle');
	return input;
}

describe('ChapterList', () => {
	let component: ReturnType<typeof mount> | null = null;

	afterEach(() => {
		if (component) unmount(component);
		component = null;
		document.body.innerHTML = '';
	});

	// The mark is set by a download and by adding the series to the library (frmMain.pas:2797-2846),
	// so it says "seen", not "downloaded".
	it('calls a marked chapter seen, and Hide seen hides it', () => {
		const chapters = [
			{ name: 'Chapter 1', link: '/c/1', downloaded: true },
			{ name: 'Chapter 2', link: '/c/2', downloaded: false }
		];
		component = mount(ChapterList, {
			target: document.body,
			props: { chapters, selected: new Set<number>() }
		});
		flushSync();

		const rows = () => [...document.querySelectorAll('.row')].map((r) => r.textContent);
		expect(rows()).toEqual([
			expect.stringMatching(/Chapter 1\s*✓ seen/),
			expect.not.stringContaining('✓')
		]);

		hideSeen().click();
		flushSync();

		expect(rows()).toEqual([expect.stringContaining('Chapter 2')]);
	});

	it('says every chapter is seen when hiding leaves none', () => {
		const chapters = [{ name: 'Chapter 1', link: '/c/1', downloaded: true }];
		component = mount(ChapterList, {
			target: document.body,
			props: { chapters, selected: new Set<number>() }
		});
		flushSync();
		hideSeen().click();
		flushSync();

		expect(document.querySelector('.empty')?.textContent?.trim()).toBe('Every chapter is seen.');
	});
});
