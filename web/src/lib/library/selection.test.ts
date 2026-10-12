import { describe, expect, it } from 'vitest';
import type { FavoriteView } from '#lib/api/types.ts';
import { emptyFilters, filterFavorites, type LibraryFilters } from '#lib/library/filters.ts';
import { inLibrary, selectShown, selectionLabel, toggle } from '#lib/library/selection.ts';

const favorite = (id: number, title: string, patch: Partial<FavoriteView> = {}): FavoriteView => ({
	id,
	module_id: 'mangadex',
	website: 'MangaDex',
	link: `/title/${id}`,
	title,
	display_title: title,
	status: 'ongoing',
	enabled: true,
	save_to: '/data',
	cover_url: `/api/covers/series?module=mangadex&link=%2Ftitle%2F${id}`,
	current_chapter: 10,
	new_chapters: 0,
	date_added: '2026-01-01T00:00:00Z',
	last_checked: null,
	last_updated: null,
	group_id: null,
	...patch
});

const library: FavoriteView[] = [
	favorite(1, 'Frieren'),
	favorite(2, 'One Piece', { status: 'completed' }),
	favorite(3, 'Blue Lock', { enabled: false }),
	favorite(4, 'Kagurabachi', { status: 'completed' })
];

const shown = (patch: Partial<LibraryFilters>): FavoriteView[] =>
	filterFavorites(library, { ...emptyFilters(), ...patch });

const ids = (selected: ReadonlySet<number>): number[] => [...selected].sort((a, b) => a - b);

describe('library selection', () => {
	it('select all picks the shown series only, keeping those already picked', () => {
		const completed = selectShown(new Set([1]), shown({ chip: 'completed' }));
		expect(ids(completed)).toEqual([1, 2, 4]);

		const searched = selectShown(new Set(), shown({ q: 'frie' }));
		expect(ids(searched)).toEqual([1]);
	});

	it('keeps series a filter hides selected, and counts them as hidden', () => {
		let selected = new Set<number>();
		for (const id of [1, 2, 3]) selected = toggle(selected, id);
		expect(selectionLabel(selected, shown({}))).toBe('3 selected');

		const completed = shown({ chip: 'completed' });
		expect(ids(selected)).toEqual([1, 2, 3]);
		expect(selectionLabel(selected, completed)).toBe('3 selected (2 hidden)');

		selected = toggle(selected, 2);
		expect(selectionLabel(selected, completed)).toBe('2 selected (2 hidden)');
	});

	it('drops series no longer in the library, so the count stays right', () => {
		const selected = new Set([1, 2, 3]);
		const left = library.filter((f) => f.id !== 2);

		const kept = inLibrary(selected, left);

		expect(ids(kept)).toEqual([1, 3]);
		expect(selectionLabel(kept, left)).toBe('2 selected');
	});
});
