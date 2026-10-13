import type { FacetValue, FavoriteView } from '#lib/api/types.ts';
import { FORMAT, UNKNOWN, type Tri } from '#lib/discover/filters.ts';

export type Chip = 'all' | 'new' | 'ongoing' | 'completed' | 'disabled';

export const CHIPS: { id: Chip; label: string }[] = [
	{ id: 'all', label: 'All' },
	{ id: 'new', label: 'New' },
	{ id: 'ongoing', label: 'Ongoing' },
	{ id: 'completed', label: 'Completed' },
	{ id: 'disabled', label: 'Paused' }
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

export interface LibraryFilters {
	chip: Chip;
	/** Module ID; empty for every website. */
	website: string;
	q: string;
	/** Tags not listed are ignored; compared without case. */
	tags: Record<string, Tri>;
	/** The MangaBaka format (`manga`, …, `unknown`); empty for any. */
	format: string;
	sort: Sort;
}

export const emptyFilters = (): LibraryFilters => ({
	chip: 'all',
	website: '',
	q: '',
	tags: {},
	format: '',
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

/** The shown title, or the website's: a series is still found by the name the website gives it. */
const matchesQuery = (f: FavoriteView, q: string): boolean =>
	f.display_title.toLowerCase().includes(q) || f.title.toLowerCase().includes(q);

/** Every included tag and none of the excluded ones, compared without case. */
const matchesTags = (f: FavoriteView, tags: Record<string, Tri>): boolean => {
	const has = new Set(f.tags.map((t) => t.toLowerCase()));
	return Object.entries(tags).every(([tag, state]) =>
		state === 'ignore' ? true : has.has(tag.toLowerCase()) === (state === 'include')
	);
};

/** A series' format; `unknown` without a MangaBaka match. */
const formatOf = (f: FavoriteView): string => f.format ?? UNKNOWN;

/** RFC 3339 times compare as strings; a missing one sorts last. */
const newestFirst = (a: string | null | undefined, b: string | null | undefined): number =>
	(b ?? '').localeCompare(a ?? '');

const COMPARE: Record<Exclude<Sort, 'library'>, (a: FavoriteView, b: FavoriteView) => number> = {
	title: (a, b) =>
		a.display_title.localeCompare(b.display_title, undefined, { sensitivity: 'base' }),
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
			(!q || matchesQuery(f, q)) &&
			matchesTags(f, filters.tags) &&
			(!filters.format || formatOf(f) === filters.format)
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

/**
 * The tags in the library with their series counts, most common first, then by name; a tag
 * spelled in several cases counts once, under its first spelling. And the formats, in
 * `FORMAT` order with `unknown` last.
 */
export function facets(list: FavoriteView[]): { tags: FacetValue[]; formats: FacetValue[] } {
	const tags = new Map<string, FacetValue>();
	const formats = new Map<string, number>();
	for (const f of list) {
		for (const tag of new Set(f.tags.map((t) => t.toLowerCase()))) {
			const first = f.tags.find((t) => t.toLowerCase() === tag) ?? tag;
			const facet = tags.get(tag) ?? { value: first, count: 0 };
			facet.count++;
			tags.set(tag, facet);
		}
		formats.set(formatOf(f), (formats.get(formatOf(f)) ?? 0) + 1);
	}
	const order = [...Object.keys(FORMAT), UNKNOWN];
	return {
		tags: [...tags.values()].sort(
			(a, b) =>
				b.count - a.count || a.value.localeCompare(b.value, undefined, { sensitivity: 'base' })
		),
		formats: [...formats.entries()]
			.map(([value, count]) => ({ value, count }))
			.sort((a, b) => rank(order, a.value) - rank(order, b.value))
	};
}

/** Where `value` is in `order`; values it lacks go last. */
const rank = (order: string[], value: string): number =>
	order.includes(value) ? order.indexOf(value) : order.length;
