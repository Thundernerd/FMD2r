import { describe, expect, it } from 'vitest';
import type { FavoriteView } from '#lib/api/types.ts';
import {
	chipCounts,
	emptyFilters,
	filterFavorites,
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
	cover_url: null,
	current_chapter: 10,
	new_chapters: 0,
	date_added: '2026-01-01T00:00:00Z',
	last_checked: null,
	last_updated: null,
	...patch
});

const library: FavoriteView[] = [
	favorite(1, { title: 'Frieren', new_chapters: 2, last_updated: '2026-10-08T09:00:00Z' }),
	favorite(2, { title: 'One Piece', status: 'completed', date_added: '2026-03-01T00:00:00Z' }),
	favorite(3, {
		title: 'Blue Lock',
		module_id: 'comick',
		website: 'ComicK',
		enabled: false,
		last_updated: '2026-10-01T00:00:00Z'
	}),
	favorite(4, { title: 'Kagurabachi', status: 'hiatus', new_chapters: 5 })
];

const titles = (patch: Partial<LibraryFilters>): string[] =>
	filterFavorites(library, { ...emptyFilters(), ...patch }).map((f) => f.title);

describe('library chips', () => {
	it('All shows everything, in library order by default', () => {
		expect(titles({})).toEqual(['Frieren', 'One Piece', 'Blue Lock', 'Kagurabachi']);
	});

	it('New shows favorites with chapters not downloaded', () => {
		expect(titles({ chip: 'new' })).toEqual(['Frieren', 'Kagurabachi']);
	});

	it('Ongoing and Completed go by the series status', () => {
		expect(titles({ chip: 'ongoing' })).toEqual(['Frieren', 'Blue Lock']);
		expect(titles({ chip: 'completed' })).toEqual(['One Piece']);
	});

	it('Disabled shows the favorites the check skips', () => {
		expect(titles({ chip: 'disabled' })).toEqual(['Blue Lock']);
	});

	it('counts each chip', () => {
		expect(chipCounts(library)).toEqual({
			all: 4,
			new: 2,
			ongoing: 2,
			completed: 1,
			disabled: 1
		});
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
