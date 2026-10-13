import { describe, expect, it } from 'vitest';
import type { FavoriteView, GroupView } from '#lib/api/types.ts';
import { emptyFilters, type LibraryFilters } from '#lib/library/filters.ts';
import { sections } from '#lib/library/sections.ts';

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
	tags: [],
	check_failures: 0,
	...patch
});

const groups: GroupView[] = [
	{ id: 20, name: 'Seinen', collapsed: true },
	{ id: 10, name: 'Shounen', collapsed: false },
	{ id: 30, name: 'Dropped', collapsed: false }
];

const library: FavoriteView[] = [
	favorite(1, 'Sakamoto Days', { group_id: 10, tags: ['Action'], format: 'manga' }),
	favorite(2, 'Frieren', { tags: ['Fantasy'], format: 'manga' }),
	favorite(3, 'Vinland Saga', { group_id: 20, status: 'completed', tags: ['Action'] }),
	favorite(4, 'Kagurabachi', { group_id: 10, new_chapters: 3, tags: ['action', 'Fantasy'] }),
	favorite(5, 'Berserk', { group_id: 20, tags: ['Fantasy'], format: 'manga' }),
	favorite(6, 'Blue Lock', { group_id: 99 })
];

const shape = (patch: Partial<LibraryFilters> = {}) =>
	sections(library, groups, { ...emptyFilters(), ...patch }).map((s) => ({
		name: s.name,
		collapsed: s.collapsed,
		titles: s.favorites.map((f) => f.title)
	}));

describe('library sections', () => {
	it('follows the group order, drops empty groups and puts Ungrouped last', () => {
		expect(shape()).toEqual([
			{ name: 'Seinen', collapsed: true, titles: ['Vinland Saga', 'Berserk'] },
			{ name: 'Shounen', collapsed: false, titles: ['Sakamoto Days', 'Kagurabachi'] },
			// A group that is gone (deleted elsewhere) counts as none.
			{ name: 'Ungrouped', collapsed: false, titles: ['Frieren', 'Blue Lock'] }
		]);
	});

	it('sorts inside each section', () => {
		expect(shape({ sort: 'title' }).map((s) => s.titles)).toEqual([
			['Berserk', 'Vinland Saga'],
			['Kagurabachi', 'Sakamoto Days'],
			['Blue Lock', 'Frieren']
		]);
	});

	it('filters inside each section and hides a section with no matches', () => {
		expect(shape({ chip: 'new' })).toEqual([
			{ name: 'Shounen', collapsed: false, titles: ['Kagurabachi'] }
		]);
		expect(shape({ q: 'saga' }).map((s) => s.name)).toEqual(['Seinen']);
	});

	it('applies the tag and format filters inside each section', () => {
		expect(shape({ tags: { Action: 'include', fantasy: 'exclude' } })).toEqual([
			{ name: 'Seinen', collapsed: true, titles: ['Vinland Saga'] },
			{ name: 'Shounen', collapsed: false, titles: ['Sakamoto Days'] }
		]);
		expect(shape({ format: 'manga', tags: { Fantasy: 'include' } })).toEqual([
			{ name: 'Seinen', collapsed: true, titles: ['Berserk'] },
			{ name: 'Ungrouped', collapsed: false, titles: ['Frieren'] }
		]);
		expect(shape({ format: 'unknown' }).map((s) => s.titles)).toEqual([
			['Vinland Saga'],
			['Kagurabachi'],
			['Blue Lock']
		]);
	});

	it('keys each section by its group, and Ungrouped by null', () => {
		expect(sections(library, groups, emptyFilters()).map((s) => s.group?.id ?? null)).toEqual([
			20,
			10,
			null
		]);
	});
});
