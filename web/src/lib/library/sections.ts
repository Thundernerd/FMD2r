import type { FavoriteView, GroupView } from '#lib/api/types.ts';
import { filterFavorites, type LibraryFilters } from '#lib/library/filters.ts';

/** A foldable part of the Library: a group's series, or the ungrouped ones. */
export interface Section {
	/** `null` for "Ungrouped". */
	group: GroupView | null;
	name: string;
	collapsed: boolean;
	/** The series `filters` select, in the order they ask for. */
	favorites: FavoriteView[];
}

export const UNGROUPED = 'Ungrouped';

/**
 * The Library's sections: each group in its order, then "Ungrouped", with `filters` applied
 * inside each and the sections they leave empty dropped. A series whose group is not in
 * `groups` counts as ungrouped.
 */
export function sections(
	favorites: FavoriteView[],
	groups: GroupView[],
	filters: LibraryFilters
): Section[] {
	const shown = filterFavorites(favorites, filters);
	const known = new Set(groups.map((g) => g.id));
	const inGroup = (f: FavoriteView): number | null =>
		f.group_id != null && known.has(f.group_id) ? f.group_id : null;
	const all: Section[] = [
		...groups.map((group) => ({
			group,
			name: group.name,
			collapsed: group.collapsed,
			favorites: shown.filter((f) => inGroup(f) === group.id)
		})),
		{
			group: null,
			name: UNGROUPED,
			collapsed: false,
			favorites: shown.filter((f) => inGroup(f) === null)
		}
	];
	return all.filter((s) => s.favorites.length > 0);
}
