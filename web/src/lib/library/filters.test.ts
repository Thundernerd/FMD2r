import { describe, expect, it } from 'vitest';
import type { FavoriteView } from '#lib/api/types.ts';
import {
	CHIPS,
	chipCounts,
	emptyFilters,
	facets,
	filterFavorites,
	isFailing,
	websites,
	type LibraryFilters
} from '#lib/library/filters.ts';

const favorite = (id: number, patch: Partial<FavoriteView> = {}): FavoriteView => ({
	id,
	module_id: 'mangadex',
	website: 'MangaDex',
	link: `/title/${id}`,
	title: `Series ${id}`,
	status: 'ongoing',
	enabled: true,
	save_to: '/data',
	cover_url: `/api/covers/series?module=mangadex&link=%2Ftitle%2F${id}`,
	current_chapter: 10,
	new_chapters: 0,
	date_added: '2026-01-01T00:00:00Z',
	last_checked: null,
	last_updated: null,
	tags: [],
	check_failures: 0,
	...patch,
	display_title: patch.display_title ?? patch.title ?? `Series ${id}`
});

const library: FavoriteView[] = [
	favorite(1, {
		title: 'Frieren',
		new_chapters: 2,
		last_updated: '2026-10-08T09:00:00Z',
		tags: ['Fantasy', 'Adventure'],
		format: 'manga'
	}),
	favorite(2, {
		title: 'One Piece',
		status: 'completed',
		date_added: '2026-03-01T00:00:00Z',
		tags: ['Adventure', 'Action'],
		format: 'manga'
	}),
	favorite(3, {
		title: 'Blue Lock',
		module_id: 'comick',
		website: 'ComicK',
		enabled: false,
		last_updated: '2026-10-01T00:00:00Z',
		tags: ['sports', 'action'],
		format: 'manhwa'
	}),
	favorite(4, { title: 'Kagurabachi', status: 'hiatus', new_chapters: 5, tags: ['Action'] })
];

const titles = (patch: Partial<LibraryFilters>): string[] =>
	filterFavorites(library, { ...emptyFilters(), ...patch }).map((f) => f.title);

describe('library chips', () => {
	it('All shows everything, in library order by default', () => {
		expect(titles({})).toEqual(['Frieren', 'One Piece', 'Blue Lock', 'Kagurabachi']);
	});

	it('New shows favorites with chapters not seen', () => {
		expect(titles({ chip: 'new' })).toEqual(['Frieren', 'Kagurabachi']);
	});

	it('Ongoing and Completed go by the series status', () => {
		expect(titles({ chip: 'ongoing' })).toEqual(['Frieren', 'Blue Lock']);
		expect(titles({ chip: 'completed' })).toEqual(['One Piece']);
	});

	it('Paused shows the favorites the check skips', () => {
		expect(CHIPS.find((c) => c.id === 'disabled')?.label).toBe('Paused');
		expect(titles({ chip: 'disabled' })).toEqual(['Blue Lock']);
	});

	it('counts each chip', () => {
		expect(chipCounts(library)).toEqual({
			all: 4,
			new: 2,
			ongoing: 2,
			completed: 1,
			disabled: 1,
			failing: 0
		});
	});

	it('Failing shows the series whose website failed 3 checks or more in a row', () => {
		const gone = favorite(5, { title: 'Gone', check_failures: 3, last_check_error: 'HTTP 404' });
		const hiccup = favorite(6, { title: 'Hiccup', check_failures: 2, last_check_error: 'timeout' });
		const list = [...library, gone, hiccup];

		expect(CHIPS.find((c) => c.id === 'failing')?.label).toBe('Failing');
		expect(
			filterFavorites(list, { ...emptyFilters(), chip: 'failing' }).map((f) => f.title)
		).toEqual(['Gone']);
		expect(chipCounts(list).failing).toBe(1);
		expect(isFailing(gone)).toBe(true);
		expect(isFailing(hiccup)).toBe(false);
	});

	it('Failing narrows together with the tag and format filters', () => {
		const manhwa = favorite(5, {
			title: 'Gone Manhwa',
			check_failures: 3,
			tags: ['Action'],
			format: 'manhwa'
		});
		const manga = favorite(6, {
			title: 'Gone Manga',
			check_failures: 5,
			tags: ['Romance'],
			format: 'manga'
		});
		const list = [...library, manhwa, manga];
		const failing = (patch: Partial<LibraryFilters>) =>
			filterFavorites(list, { ...emptyFilters(), chip: 'failing', ...patch }).map((f) => f.title);

		expect(failing({})).toEqual(['Gone Manhwa', 'Gone Manga']);
		expect(failing({ format: 'manhwa' })).toEqual(['Gone Manhwa']);
		expect(failing({ tags: { romance: 'include' } })).toEqual(['Gone Manga']);
		expect(failing({ tags: { action: 'exclude' } })).toEqual(['Gone Manga']);
	});
});

describe('website chips', () => {
	it('list each website once with its count, by name', () => {
		expect(websites(library)).toEqual([
			{ id: 'comick', name: 'ComicK', count: 1 },
			{ id: 'mangadex', name: 'MangaDex', count: 3 }
		]);
	});

	it('narrow to one website, together with the state chip', () => {
		expect(titles({ website: 'mangadex' })).toEqual(['Frieren', 'One Piece', 'Kagurabachi']);
		expect(titles({ website: 'mangadex', chip: 'new' })).toEqual(['Frieren', 'Kagurabachi']);
	});
});

describe('search and sort', () => {
	it('search matches part of the title, ignoring case and surrounding space', () => {
		expect(titles({ q: '  LOCK ' })).toEqual(['Blue Lock']);
		expect(titles({ q: 'zzz' })).toEqual([]);
	});

	it('sorts by title', () => {
		expect(titles({ sort: 'title' })).toEqual(['Blue Lock', 'Frieren', 'Kagurabachi', 'One Piece']);
	});

	it('sorts by new chapters, most first, keeping library order for ties', () => {
		expect(titles({ sort: 'new' })).toEqual(['Kagurabachi', 'Frieren', 'One Piece', 'Blue Lock']);
	});

	it('sorts by last update, newest first, never-updated last', () => {
		expect(titles({ sort: 'updated' })).toEqual([
			'Frieren',
			'Blue Lock',
			'One Piece',
			'Kagurabachi'
		]);
	});

	it('sorts by date added, newest first', () => {
		expect(titles({ sort: 'added' })[0]).toBe('One Piece');
	});
});

describe('tag chips', () => {
	it('an included tag keeps the series that have it', () => {
		expect(titles({ tags: { Adventure: 'include' } })).toEqual(['Frieren', 'One Piece']);
	});

	it('a series needs every included tag', () => {
		expect(titles({ tags: { Adventure: 'include', Action: 'include' } })).toEqual(['One Piece']);
	});

	it('an excluded tag drops the series that have it', () => {
		expect(titles({ tags: { Action: 'exclude' } })).toEqual(['Frieren']);
		expect(titles({ tags: { Adventure: 'include', Action: 'exclude' } })).toEqual(['Frieren']);
	});

	it('compares tags without case', () => {
		expect(titles({ tags: { SPORTS: 'include' } })).toEqual(['Blue Lock']);
		expect(titles({ tags: { action: 'include' } })).toEqual([
			'One Piece',
			'Blue Lock',
			'Kagurabachi'
		]);
	});

	it('ignored tags change nothing', () => {
		expect(titles({ tags: { Action: 'ignore' } })).toEqual(titles({}));
	});
});

describe('format', () => {
	it('keeps the series of exactly that format', () => {
		expect(titles({ format: 'manga' })).toEqual(['Frieren', 'One Piece']);
		expect(titles({ format: 'manhwa' })).toEqual(['Blue Lock']);
		expect(titles({ format: 'manhua' })).toEqual([]);
	});

	it('Unknown keeps the series without a MangaBaka match', () => {
		expect(titles({ format: 'unknown' })).toEqual(['Kagurabachi']);
	});

	it('combines with the chip, website, search and tags', () => {
		expect(titles({ format: 'manga', chip: 'new' })).toEqual(['Frieren']);
		expect(titles({ format: 'manga', website: 'comick' })).toEqual([]);
		expect(titles({ format: 'manga', q: 'piece' })).toEqual(['One Piece']);
		expect(titles({ format: 'manga', tags: { Action: 'include' } })).toEqual(['One Piece']);
		expect(titles({ chip: 'new', tags: { Action: 'include' } })).toEqual(['Kagurabachi']);
		expect(titles({ website: 'comick', tags: { Action: 'exclude' } })).toEqual([]);
	});
});

describe('tag and format counts', () => {
	it('count each tag once per series ignoring case, most common first, then by name', () => {
		expect(facets(library).tags).toEqual([
			{ value: 'Action', count: 3 },
			{ value: 'Adventure', count: 2 },
			{ value: 'Fantasy', count: 1 },
			{ value: 'sports', count: 1 }
		]);
	});

	it('count each format in the order MangaBaka lists them, unknown last', () => {
		expect(facets(library).formats).toEqual([
			{ value: 'manga', count: 2 },
			{ value: 'manhwa', count: 1 },
			{ value: 'unknown', count: 1 }
		]);
	});
});
