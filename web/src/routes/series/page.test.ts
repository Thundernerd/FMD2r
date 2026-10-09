// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, tick, unmount } from 'svelte';
import { ApiError, type Api } from '#lib/api/client.ts';
import type { SeriesInfo } from '#lib/api/types.ts';

// jsdom has no ResizeObserver; the chapter list's `bind:clientHeight` needs one.
globalThis.ResizeObserver ??= class {
	observe() {}
	unobserve() {}
	disconnect() {}
};

const getSeries = vi.fn<(module: string, link: string) => Promise<SeriesInfo>>();

vi.mock('$app/state', () => ({
	page: { url: new URL('http://localhost/series?module=mangadex&link=%2Ftitle%2Ffrieren') }
}));
vi.mock('#lib/app.ts', () => {
	// What the page calls before anyone presses a button.
	const api: Pick<Api, 'getSeries' | 'listModules' | 'getSettings'> = {
		getSeries: (module, link) => getSeries(module, link),
		listModules: () => Promise.resolve([]),
		getSettings: () => new Promise(() => {})
	};
	return { api, events: { queue: { upsert: () => {} } } };
});

const { default: SeriesPage } = await import('./+page.svelte');

const frieren: SeriesInfo = {
	module_id: 'mangadex',
	link: '/title/frieren',
	title: 'Frieren',
	alt_titles: '',
	authors: '',
	artists: '',
	genres: [],
	summary: '',
	status: 'ongoing',
	in_library: false,
	chapters: [{ name: 'Chapter 1', link: '/c/1', downloaded: false }]
};

/** A promise and the functions that settle it. */
function deferred<T>() {
	let resolve!: (value: T) => void;
	let reject!: (reason: unknown) => void;
	const promise = new Promise<T>((res, rej) => {
		resolve = res;
		reject = rej;
	});
	return { promise, resolve, reject };
}

/** Lets the page react to a settled `getSeries`. */
async function settle() {
	await tick();
	await Promise.resolve();
	flushSync();
}

describe('series page', () => {
	let component: ReturnType<typeof mount> | null = null;
	let pending: ReturnType<typeof deferred<SeriesInfo>>;

	beforeEach(() => {
		pending = deferred<SeriesInfo>();
		getSeries.mockReturnValue(pending.promise);
		component = mount(SeriesPage, { target: document.body });
		flushSync();
	});

	afterEach(() => {
		if (component) unmount(component);
		component = null;
		document.body.innerHTML = '';
	});

	const skeleton = () => document.querySelector('[aria-busy="true"]');
	const chapterList = () => document.querySelector('section[aria-label="Chapters"]');

	it('shows a busy skeleton, announced to screen readers, while the series loads', () => {
		expect(skeleton()).not.toBeNull();
		expect(document.querySelector('[aria-live]')?.textContent).toContain('Loading series…');
		expect(chapterList()).toBeNull();
		expect(document.querySelector('a[href="/"]')?.textContent).toContain('Library');
	});

	it('replaces the skeleton with the series once it loads', async () => {
		pending.resolve(frieren);
		await settle();

		expect(skeleton()).toBeNull();
		expect(document.querySelector('h1')?.textContent).toBe('Frieren');
		expect(chapterList()).not.toBeNull();
	});

	it('replaces the skeleton with the error when the series is not found', async () => {
		pending.reject(new ApiError(404, 'GET /api/series', 'series not found'));
		await settle();

		expect(skeleton()).toBeNull();
		expect(document.querySelector('[role="alert"]')?.textContent).toContain(
			'This series was not found'
		);
	});
});
