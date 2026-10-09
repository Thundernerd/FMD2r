// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, tick, unmount } from 'svelte';
import { ApiError, type Api } from '#lib/api/client.ts';
import type { ModuleSettingsView, SeriesInfo, Settings } from '#lib/api/types.ts';

// jsdom has no ResizeObserver; the chapter list's `bind:clientHeight` needs one.
globalThis.ResizeObserver ??= class {
	observe() {}
	unobserve() {}
	disconnect() {}
};

const getSeries = vi.fn<(module: string, link: string) => Promise<SeriesInfo>>();
const getSettings = vi.fn<() => Promise<Settings>>();
const getModuleSettings = vi.fn<(id: string) => Promise<ModuleSettingsView>>();
const saveFolder = vi.fn<Api['saveFolder']>();

vi.mock('$app/state', () => ({
	page: { url: new URL('http://localhost/series?module=mangadex&link=%2Ftitle%2Ffrieren') }
}));
vi.mock('#lib/app.ts', () => {
	// What the page calls before anyone presses a button.
	const api: Pick<
		Api,
		'getSeries' | 'listModules' | 'getSettings' | 'getModuleSettings' | 'saveFolder'
	> = {
		getSeries: (module, link) => getSeries(module, link),
		listModules: () => Promise.resolve([]),
		getSettings: () => getSettings(),
		getModuleSettings: (id) => getModuleSettings(id),
		saveFolder: (request) => saveFolder(request)
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
	summary_from_mangabaka: false,
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
		getSettings.mockReturnValue(new Promise(() => {}));
		getModuleSettings.mockReturnValue(new Promise(() => {}));
		saveFolder.mockReturnValue(new Promise(() => {}));
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

/** Settings with two destinations, Manga the default, as the server sends them. */
async function withDestinations(): Promise<Settings> {
	const { defaultSettings } = await import('#lib/api/mock-settings.ts');
	const settings = defaultSettings();
	settings.saveto.destinations = [
		{ name: 'Manga', path: '/data/manga', default: true },
		{ name: 'Manhwa', path: '/data/manhwa', default: false }
	];
	settings.saveto.default_dir = '/data/manga';
	return settings;
}

/** The module settings of `mangadex` with its download folder `saveTo`. */
const websiteFolder = (saveTo: string) =>
	({ id: 'mangadex', name: 'MangaDex', save_to: saveTo }) as ModuleSettingsView;

describe('the series page download folder', () => {
	let component: ReturnType<typeof mount> | null = null;

	afterEach(() => {
		if (component) unmount(component);
		component = null;
		document.body.innerHTML = '';
	});

	async function open(website: string) {
		getSeries.mockResolvedValue(frieren);
		getSettings.mockResolvedValue(await withDestinations());
		getModuleSettings.mockResolvedValue(websiteFolder(website));
		saveFolder.mockImplementation(async ({ save_to, title }) => `${save_to}/${title}`);
		component = mount(SeriesPage, { target: document.body });
		await shown();
	}

	/** Lets the page load and show the folder, which it asks for after a pause in typing. */
	async function shown() {
		for (let i = 0; i < 5; i++) await settle();
		await new Promise((resolve) => setTimeout(resolve, 300));
		for (let i = 0; i < 3; i++) await settle();
	}

	const picker = () => document.querySelector<HTMLSelectElement>('select[aria-label="Save to"]');
	const pickedName = () => picker()?.selectedOptions[0]?.textContent?.trim();
	const customBox = () =>
		document.querySelector<HTMLInputElement>('input[aria-label="Custom folder"]');

	it("starts on the website's destination when it has one", async () => {
		await open('/data/manhwa');
		expect(pickedName()).toBe('Manhwa');
		expect(customBox()).toBeNull();
		// The full folder, the manga folder included, shows under the picker.
		expect(document.body.textContent).toContain('/data/manhwa/Frieren');
	});

	it('starts on the default destination otherwise', async () => {
		await open('');
		expect(pickedName()).toBe('Manga (default)');
		expect(document.body.textContent).toContain('/data/manga/Frieren');
	});

	it('shows the free-text box for "Custom folder…"', async () => {
		await open('');
		const select = picker();
		if (!select) throw new Error('no picker');
		select.value = 'custom';
		select.dispatchEvent(new Event('change', { bubbles: true }));
		await settle();
		const box = customBox();
		expect(box?.value).toBe('/data/manga');
		if (!box) throw new Error('no box');
		box.value = '/srv/elsewhere';
		box.dispatchEvent(new Event('input', { bubbles: true }));
		await shown();
		expect(document.body.textContent).toContain('/srv/elsewhere/Frieren');
	});
});
