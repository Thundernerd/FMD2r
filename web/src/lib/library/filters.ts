import type { FavoriteView } from '#lib/api/types.ts';

/** The Library's state chips. */
export type Chip = 'all' | 'new' | 'ongoing' | 'completed' | 'disabled';

export const CHIPS: { id: Chip; label: string }[] = [
	{ id: 'all', label: 'All' },
	{ id: 'new', label: 'New' },
	{ id: 'ongoing', label: 'Ongoing' },
	{ id: 'completed', label: 'Completed' },
	{ id: 'disabled', label: 'Disabled' }
];

/** How the grid is ordered; `library` keeps the server's order. */
export type Sort = 'library' | 'title' | 'new' | 'updated' | 'added';

export const SORTS: { id: Sort; label: string }[] = [
	{ id: 'library', label: 'Library order' },
	{ id: 'title', label: 'Title' },
	{ id: 'new', label: 'New chapters' },
	{ id: 'updated', label: 'Last updated' },
	{ id: 'added', label: 'Date added' }
];

/** What the Library shows. */
export interface LibraryFilters {
	chip: Chip;
	/** Module ID; empty for every website. */
	website: string;
	q: string;
	sort: Sort;
}

export const emptyFilters = (): LibraryFilters => ({
	chip: 'all',
	website: '',
	q: '',
	sort: 'library'
});

const matchesChip = (f: FavoriteView, chip: Chip): boolean => {
	switch (chip) {
		case 'all':
			return true;
		case 'new':
			return f.new_chapters > 0;
		case 'ongoing':
			return f.status === 'ongoing';
		case 'completed':
			return f.status === 'completed';
		case 'disabled':
			return !f.enabled;
	}
};

/** RFC 3339 times compare as strings; a missing one sorts last. */
const newestFirst = (a: string | null | undefined, b: string | null | undefined): number =>
	(b ?? '').localeCompare(a ?? '');

const COMPARE: Record<Exclude<Sort, 'library'>, (a: FavoriteView, b: FavoriteView) => number> = {
	title: (a, b) => a.title.localeCompare(b.title, undefined, { sensitivity: 'base' }),
	new: (a, b) => b.new_chapters - a.new_chapters,
	updated: (a, b) => newestFirst(a.last_updated, b.last_updated),
	added: (a, b) => newestFirst(a.date_added, b.date_added)
};

/** The favorites `filters` select, in the order they ask for. */
export function filterFavorites(list: FavoriteView[], filters: LibraryFilters): FavoriteView[] {
	const q = filters.q.trim().toLowerCase();
	const shown = list.filter(
		(f) =>
			matchesChip(f, filters.chip) &&
			(!filters.website || f.module_id === filters.website) &&
			(!q || f.title.toLowerCase().includes(q))
	);
	// `sort` is stable, so ties keep the library order.
	return filters.sort === 'library' ? shown : shown.sort(COMPARE[filters.sort]);
}

/** How many favorites each state chip shows. */
export function chipCounts(list: FavoriteView[]): Record<Chip, number> {
	const counts = { all: 0, new: 0, ongoing: 0, completed: 0, disabled: 0 };
	for (const f of list) {
		for (const { id } of CHIPS) if (matchesChip(f, id)) counts[id]++;
	}
	return counts;
}

/** The websites in the library with their favorite counts, by name. */
export function websites(list: FavoriteView[]): { id: string; name: string; count: number }[] {
	const byId = new Map<string, { id: string; name: string; count: number }>();
	for (const f of list) {
		const site = byId.get(f.module_id) ?? { id: f.module_id, name: f.website, count: 0 };
		site.count++;
		byId.set(f.module_id, site);
	}
	return [...byId.values()].sort((a, b) => a.name.localeCompare(b.name));
}
