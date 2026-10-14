import type {
	BulkChange,
	FavoritePatch,
	FavoriteView,
	FavoritesEvent,
	GroupPatch,
	GroupView,
	InboxItem,
	JobState,
	LibraryTitle,
	RuleView,
	SeriesInfo
} from './types';
import { seriesCover } from './mock-lists';
import { FAILING_CHECKS } from '#lib/library/filters.ts';
import { FORMATS } from '#lib/library/formats.ts';

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
		// FMD2's favorites have no cover, so the server points at the series cover.
		cover_url: seriesCover(module_id, link),
		display_title: title,
		current_chapter: 40,
		new_chapters: 0,
		date_added: `2026-0${(id % 9) + 1}-01T00:00:00Z`,
		last_checked: '2026-10-08T09:12:00Z',
		last_updated: null,
		group_id: null,
		tags: [],
		format: null,
		check_failures: 0,
		last_check_error: null,
		...patch
	});
	return [
		favorite(1, 'Frieren', 'mangadex', 'MangaDex', '/title/abc123/frieren', {
			// Added from its series page, so its cover is stored and goes through the proxy.
			cover_url: `/api/covers?${new URLSearchParams({ module: 'mangadex', url: 'https://mangadex.test/frieren.jpg' })}`,
			current_chapter: 142,
			new_chapters: 4,
			last_updated: '2026-10-08T09:12:00Z',
			// The website's genres, then the rest of its MangaBaka match's.
			tags: ['Adventure', 'Drama', 'Fantasy', 'Shounen'],
			format: 'manga'
		}),
		favorite(2, 'Kagurabachi', 'mangadex', 'MangaDex', '/title/kgb/kagurabachi', {
			current_chapter: 98,
			new_chapters: 2,
			last_updated: '2026-10-08T09:12:00Z',
			// Unmatched: the website's genres only, and no format.
			tags: ['Action', 'Supernatural']
		}),
		favorite(3, 'Omniscient Reader’s Viewpoint', 'comick', 'ComicK', '/comic/orv', {
			current_chapter: 241,
			new_chapters: 3,
			last_updated: '2026-10-07T18:00:00Z',
			tags: ['Action', 'Fantasy', 'Apocalypse'],
			format: 'manhwa'
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
			current_chapter: 76,
			// The website took the series down: its checks keep failing.
			last_checked: '2026-09-30T09:12:00Z',
			check_failures: 4,
			last_check_error: 'HTTP 404 Not Found'
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

/** The titles MangaBaka's database has for the favorites the mock matched, by link. */
const MANGABAKA_TITLES: Record<
	string,
	Partial<Record<Exclude<LibraryTitle, 'website'>, string>>
> = {
	'/title/abc123/frieren': {
		english: 'Frieren: Beyond Journey’s End',
		romanized: 'Sousou no Frieren',
		native: '葬送のフリーレン',
		mangabaka: 'Frieren: Beyond Journey’s End'
	},
	// No native title: the main title shows instead.
	'/comic/orv': {
		english: 'Omniscient Reader',
		romanized: 'Jeonjijeok Dokja Sijeom',
		mangabaka: 'Omniscient Reader'
	}
};

/** The title the Library shows, as fmd-server picks it (`appearance.library_title`). */
function displayTitle(favorite: FavoriteView, choice: LibraryTitle): string {
	const titles = MANGABAKA_TITLES[favorite.link];
	if (choice === 'website' || !titles) return favorite.title;
	return titles[choice] ?? titles.mangabaka ?? favorite.title;
}

/**
 * The groups and which favorite is in which, kept in `sessionStorage` so they outlive a reload,
 * as fmd-server's do.
 */
interface StoredGroups {
	groups: GroupView[];
	nextId: number;
	/** Group id by favorite id. */
	members: Record<number, number>;
}

/** Where the mock keeps {@link StoredGroups}; absent in unit tests. */
export interface GroupStorage {
	get(): string | null;
	set(value: string): void;
}

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
	/** Stores `cover` (a `SeriesInfo.cover_url`) as the cover of the favorite at `link`, if any. */
	storeCover(module: string, link: string, cover: string): void;
	patch(id: number, patch: FavoritePatch): Result;
	remove(id: number): Result;
	/**
	 * Changes or removes every favorite in `change.ids`, all or none, as fmd-server does;
	 * `folder` makes a series' manga folder in a destination, as `POST /api/save-folder` does.
	 */
	bulk(change: BulkChange, folder: (f: FavoriteView, destination: string) => string): Result;
	/** Starts a check of `ids` (every enabled favorite when `null`); false when one runs. */
	start(ids: number[] | null, mode: FavoritesEvent['mode']): boolean;
	cancel(): boolean;
	/** Checks the next favorite, reporting through `emit`; returns an inbox item when a check found chapters. */
	tick(emit: (type: string, payload: unknown) => void): InboxItem | null;
	listGroups(): GroupView[];
	createGroup(name: unknown, rule?: RuleView | null): Result;
	patchGroup(id: number, patch: GroupPatch): Result;
	deleteGroup(id: number): Result;
	reorderGroups(ids: number[]): Result;
}

/**
 * `job` is the `favorites` entry of the jobs list, kept in step with the check; `libraryTitle`
 * reads the `appearance.library_title` setting; `storage` keeps the groups across reloads.
 */
export function createMockFavorites(
	job: JobState,
	libraryTitle: () => LibraryTitle,
	storage?: GroupStorage
): MockFavorites {
	const favorites = seedFavorites();
	const fresh = seedFresh();
	let nextId = 100;
	let check: Check | null = null;

	const loadGroups = (): StoredGroups => {
		try {
			const saved = storage?.get();
			if (saved) return JSON.parse(saved) as StoredGroups;
		} catch {
			// Start without groups.
		}
		return { groups: [], nextId: 1, members: {} };
	};
	const grouping = loadGroups();
	for (const f of favorites) f.group_id = grouping.members[f.id] ?? null;
	const saveGroups = () => {
		grouping.members = {};
		for (const f of favorites) if (f.group_id != null) grouping.members[f.id] = f.group_id;
		storage?.set(JSON.stringify(grouping));
	};
	const problem = (status: number, detail: string): Result => ({
		status,
		body: { status, detail }
	});
	/** Like fmd-server: names are trimmed, required, and unique ignoring case. */
	const nameProblem = (name: string, own: number | null): Result | null => {
		if (!name) return problem(400, 'the group name cannot be empty');
		const other = grouping.groups.find(
			(g) => g.id !== own && g.name.toLowerCase() === name.toLowerCase()
		);
		return other ? problem(409, `a group named ${other.name} exists already`) : null;
	};

	/** Like fmd-server: a tag is trimmed and required, a format one of MangaBaka's five. */
	const ruleProblem = (rule: RuleView | null): Result | null => {
		if (rule?.kind === 'tag' && !rule.value) return problem(400, "the rule's tag cannot be empty");
		if (rule?.kind === 'format' && !FORMATS.some(([value]) => value === rule.value)) {
			return problem(400, `${rule.value} is not a format`);
		}
		return null;
	};
	const trimmed = (rule: RuleView | null | undefined): RuleView | null =>
		rule ? { kind: rule.kind, value: rule.value.trim() } : null;
	const matches = (rule: RuleView, f: FavoriteView) =>
		rule.kind === 'tag'
			? f.tags.some((t) => t.toLowerCase() === rule.value.toLowerCase())
			: f.format === rule.value;
	/** Like fmd-server: each series without a group goes to the first group whose rule matches. */
	const autoGroup = (list: FavoriteView[]) => {
		for (const f of list) {
			if (f.group_id != null) continue;
			const group = grouping.groups.find((g) => g.rule && matches(g.rule, f));
			if (group) f.group_id = group.id;
		}
		saveGroups();
	};

	const view = (f: FavoriteView): FavoriteView => ({
		...f,
		display_title: displayTitle(f, libraryTitle())
	});

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
		list: () => favorites.map(view),
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
				cover_url: series.cover_url ?? seriesCover(series.module_id, series.link),
				display_title: series.title,
				current_chapter: series.chapters.length,
				new_chapters: 0,
				date_added: new Date().toISOString(),
				last_checked: null,
				last_updated: null,
				group_id: null,
				// Not matched yet: the website's genres, without case duplicates.
				tags: series.genres.filter(
					(g, i) => series.genres.findIndex((h) => h.toLowerCase() === g.toLowerCase()) === i
				),
				format: null,
				check_failures: 0,
				last_check_error: null
			};
			favorites.push(favorite);
			autoGroup([favorite]);
			return { status: 201, body: view(favorite) };
		},
		storeCover(module, link, cover) {
			const favorite = favorites.find((f) => f.module_id === module && f.link === link);
			if (favorite) favorite.cover_url = cover;
		},
		patch(id, patch) {
			const favorite = favorites.find((f) => f.id === id);
			if (!favorite) return { status: 404 };
			favorite.enabled = patch.enabled ?? favorite.enabled;
			favorite.title = patch.title ?? favorite.title;
			favorite.save_to = patch.save_to ?? favorite.save_to;
			if (patch.group_id !== undefined) {
				if (patch.group_id !== null && !grouping.groups.some((g) => g.id === patch.group_id)) {
					return problem(400, `no group with id ${patch.group_id}`);
				}
				favorite.group_id = patch.group_id;
				saveGroups();
			}
			return { status: 200, body: view(favorite) };
		},
		remove(id) {
			const index = favorites.findIndex((f) => f.id === id);
			if (index < 0) return { status: 404 };
			favorites.splice(index, 1);
			saveGroups();
			return { status: 204 };
		},
		bulk(change, folder) {
			const picked = change.ids.map((id) => favorites.find((f) => f.id === id));
			const missing = change.ids.find((_, i) => !picked[i]);
			if (missing !== undefined) return problem(400, `no favorite with id ${missing}`);
			const list = picked.filter((f) => f !== undefined);
			const { enabled, group_id, destination, mark_seen, move_files, remove } = change;
			if (remove) {
				if (
					enabled != null ||
					group_id !== undefined ||
					destination != null ||
					mark_seen ||
					move_files
				) {
					return problem(400, 'remove cannot be combined with a change');
				}
				for (const f of list) favorites.splice(favorites.indexOf(f), 1);
				saveGroups();
				return { status: 200, body: [] };
			}
			if (destination != null && !destination.trim()) {
				return { status: 422, body: { status: 422, detail: 'the destination cannot be empty' } };
			}
			if (group_id != null && !grouping.groups.some((g) => g.id === group_id)) {
				return problem(400, `no group with id ${group_id}`);
			}
			for (const f of list) {
				f.enabled = enabled ?? f.enabled;
				if (group_id !== undefined) f.group_id = group_id;
				if (destination != null) f.save_to = folder(f, destination.trim());
				// Every chapter of the last check is marked seen.
				if (mark_seen) f.new_chapters = 0;
			}
			saveGroups();
			return { status: 200, body: list.map(view) };
		},
		listGroups: () => grouping.groups.map((g) => ({ ...g })),
		createGroup(raw, rawRule) {
			const name = typeof raw === 'string' ? raw.trim() : '';
			const rule = trimmed(rawRule);
			const refused = nameProblem(name, null) ?? ruleProblem(rule);
			if (refused) return refused;
			const group: GroupView = { id: grouping.nextId++, name, collapsed: false, rule };
			grouping.groups.push(group);
			autoGroup(favorites);
			return { status: 201, body: { ...group } };
		},
		patchGroup(id, patch) {
			const group = grouping.groups.find((g) => g.id === id);
			if (!group) return { status: 404 };
			if (patch.name != null) {
				const name = patch.name.trim();
				const refused = nameProblem(name, id);
				if (refused) return refused;
				group.name = name;
			}
			// Like fmd-server: only a rule set or changed collects series.
			let ruleSet = false;
			if (patch.rule !== undefined) {
				const rule = trimmed(patch.rule);
				const refused = ruleProblem(rule);
				if (refused) return refused;
				ruleSet =
					rule != null && (rule.kind !== group.rule?.kind || rule.value !== group.rule.value);
				group.rule = rule;
			}
			group.collapsed = patch.collapsed ?? group.collapsed;
			if (ruleSet) autoGroup(favorites);
			saveGroups();
			return { status: 200, body: { ...group } };
		},
		deleteGroup(id) {
			const index = grouping.groups.findIndex((g) => g.id === id);
			if (index < 0) return { status: 404 };
			grouping.groups.splice(index, 1);
			for (const f of favorites) if (f.group_id === id) f.group_id = null;
			saveGroups();
			return { status: 204 };
		},
		reorderGroups(ids) {
			const first = ids.flatMap((id) => grouping.groups.filter((g) => g.id === id));
			grouping.groups = [...first, ...grouping.groups.filter((g) => !first.includes(g))];
			autoGroup(favorites);
			return { status: 204 };
		},
		start(ids, mode) {
			if (check) return false;
			// A check of every favorite leaves a failing series alone, as fmd-server does for a day.
			const queue = favorites.filter(
				(f) => f.enabled && (ids === null ? f.check_failures < FAILING_CHECKS : ids.includes(f.id))
			);
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
				if (favorite.last_check_error) {
					// Its website is gone: the check fails again.
					favorite.check_failures++;
				} else {
					favorite.last_checked = new Date().toISOString();
				}
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
