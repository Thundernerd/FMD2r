import type {
	FavoritePatch,
	FavoriteView,
	FavoritesEvent,
	InboxItem,
	JobState,
	SeriesInfo
} from './types';

// The library of the in-memory backend (see mock.ts), with a favorites check that works
// through one favorite per tick.

const seedFavorites = (): FavoriteView[] => {
	const favorite = (
		id: number,
		title: string,
		module_id: string,
		website: string,
		link: string,
		patch: Partial<FavoriteView> = {}
	): FavoriteView => ({
		id,
		module_id,
		website,
		link,
		title,
		status: 'ongoing',
		enabled: true,
		save_to: `/data/downloads/${title}`,
		cover_url: null,
		current_chapter: 40,
		new_chapters: 0,
		date_added: `2026-0${(id % 9) + 1}-01T00:00:00Z`,
		last_checked: '2026-10-08T09:12:00Z',
		last_updated: null,
		...patch
	});
	return [
		favorite(1, 'Frieren', 'mangadex', 'MangaDex', '/title/abc123/frieren', {
			current_chapter: 142,
			new_chapters: 4,
			last_updated: '2026-10-08T09:12:00Z'
		}),
		favorite(2, 'Kagurabachi', 'mangadex', 'MangaDex', '/title/kgb/kagurabachi', {
			current_chapter: 98,
			new_chapters: 2,
			last_updated: '2026-10-08T09:12:00Z'
		}),
		favorite(3, 'Omniscient Reader’s Viewpoint', 'comick', 'ComicK', '/comic/orv', {
			current_chapter: 241,
			new_chapters: 3,
			last_updated: '2026-10-07T18:00:00Z'
		}),
		favorite(4, 'Sakamoto Days', 'mangadex', 'MangaDex', '/title/sd/sakamoto-days', {
			current_chapter: 190
		}),
		favorite(5, 'Blue Lock', 'comick', 'ComicK', '/comic/blue-lock', {
			enabled: false,
			current_chapter: 300
		}),
		favorite(6, 'The Apothecary Diaries', 'webtoons', 'Webtoons', '/en/apothecary', {
			status: 'hiatus',
			current_chapter: 76
		}),
		favorite(7, 'Vinland Saga', 'mangadex', 'MangaDex', '/title/vs/vinland-saga', {
			status: 'completed',
			current_chapter: 220
		}),
		favorite(8, 'Chainsaw Man', 'batoto', 'Bato.to', '/series/csm', {
			current_chapter: 180
		})
	];
};

/** What the next check finds, by favorite id. */
const seedFresh = (): Record<number, number> => ({ 8: 1 });

type Result = { status: number; body?: unknown };

/** A running check. */
interface Check {
	mode: FavoritesEvent['mode'];
	queue: FavoriteView[];
	done: number;
	total: number;
	found: { favorite: FavoriteView; chapters: number }[];
}

export interface MockFavorites {
	list(): FavoriteView[];
	has(module: string, link: string): boolean;
	add(series: SeriesInfo, website: string, saveTo: string): Result;
	patch(id: number, patch: FavoritePatch): Result;
	remove(id: number): Result;
	/** Starts a check of `ids` (every enabled favorite when `null`); false when one runs. */
	start(ids: number[] | null, mode: FavoritesEvent['mode']): boolean;
	cancel(): boolean;
	/** Checks the next favorite, reporting through `emit`; returns an inbox item when a check found chapters. */
	tick(emit: (type: string, payload: unknown) => void): InboxItem | null;
}

/** `job` is the `favorites` entry of the jobs list, kept in step with the check. */
export function createMockFavorites(job: JobState): MockFavorites {
	const favorites = seedFavorites();
	const fresh = seedFresh();
	let nextId = 100;
	let check: Check | null = null;

	const event = (kind: FavoritesEvent['kind'], c: Check): FavoritesEvent => ({
		kind,
		mode: c.mode,
		done: c.done,
		total: c.total,
		favorite_id: null,
		new_chapters: null,
		error: null
	});

	return {
		list: () => favorites,
		has: (module, link) => favorites.some((f) => f.module_id === module && f.link === link),
		add(series, website, saveTo) {
			if (favorites.some((f) => f.module_id === series.module_id && f.link === series.link)) {
				return { status: 409, body: { status: 409, detail: `${series.title} is in the library` } };
			}
			const favorite: FavoriteView = {
				id: nextId++,
				module_id: series.module_id,
				website,
				link: series.link,
				title: series.title,
				status: series.status,
				enabled: true,
				save_to: saveTo,
				cover_url: series.cover_url ?? null,
				current_chapter: series.chapters.length,
				new_chapters: 0,
				date_added: new Date().toISOString(),
				last_checked: null,
				last_updated: null
			};
			favorites.push(favorite);
			return { status: 201, body: favorite };
		},
		patch(id, patch) {
			const favorite = favorites.find((f) => f.id === id);
			if (!favorite) return { status: 404 };
			favorite.enabled = patch.enabled ?? favorite.enabled;
			favorite.title = patch.title ?? favorite.title;
			favorite.save_to = patch.save_to ?? favorite.save_to;
			return { status: 200, body: favorite };
		},
		remove(id) {
			const index = favorites.findIndex((f) => f.id === id);
			if (index < 0) return { status: 404 };
			favorites.splice(index, 1);
			return { status: 204 };
		},
		start(ids, mode) {
			if (check) return false;
			const queue = favorites.filter((f) => f.enabled && (ids === null || ids.includes(f.id)));
			check = { mode, queue, done: 0, total: queue.length, found: [] };
			Object.assign(job, {
				state: 'running',
				done: 0,
				total: queue.length,
				last_run: new Date().toISOString(),
				last_error: null
			});
			return true;
		},
		cancel() {
			if (!check) return false;
			check.queue = [];
			return true;
		},
		tick(emit) {
			if (!check) return null;
			const c = check;
			if (c.done === 0 && c.queue.length === c.total)
				emit('job.favorites.started', event('started', c));
			const favorite = c.queue.shift();
			if (favorite) {
				c.done++;
				favorite.last_checked = new Date().toISOString();
				const chapters = c.mode === 'new' ? (fresh[favorite.id] ?? 0) : 0;
				if (chapters > 0) {
					delete fresh[favorite.id];
					favorite.current_chapter += chapters;
					favorite.new_chapters += chapters;
					favorite.last_updated = favorite.last_checked;
					c.found.push({ favorite, chapters });
				}
				emit('job.favorites.progress', { ...event('progress', c), favorite_id: favorite.id });
			}
			job.done = c.done;
			if (c.queue.length > 0) {
				emit('job.state', job);
				return null;
			}
			check = null;
			const cancelled = c.done < c.total;
			job.state = cancelled ? 'idle' : 'done';
			emit('job.state', job);
			const found = c.found.reduce((n, f) => n + f.chapters, 0);
			emit(
				cancelled ? 'job.favorites.cancelled' : 'job.favorites.finished',
				cancelled ? event('cancelled', c) : { ...event('finished', c), new_chapters: found }
			);
			if (cancelled || found === 0) return null;
			// Worded as fmd-server's inbox item (and FMD2's new-chapter dialog).
			const lines = c.found.map(
				(f) => `- ${f.favorite.title} <${f.favorite.website}> has ${f.chapters} new chapter(s).`
			);
			return {
				id: `new-chapters-${Date.now()}`,
				kind: 'info',
				title: 'Found new chapter(s)',
				body: [`Found ${found} new chapter from ${c.found.length} manga(s):`, ...lines].join('\n'),
				created_at: new Date().toISOString(),
				read: false
			};
		}
	};
}
