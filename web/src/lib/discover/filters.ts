import type { paths } from '#lib/api/schema.d.ts';

/** The query of `GET /api/lists/search`. */
export type SearchQuery = NonNullable<paths['/api/lists/search']['get']['parameters']['query']>;
/** The query of `GET /api/lists/facets`. */
export type FacetQuery = NonNullable<paths['/api/lists/facets']['get']['parameters']['query']>;

/**
 * A genre chip's state, as FMD2's filter genre check boxes have it: checked (must occur),
 * unchecked (must not occur) or grayed (ignored).
 */
export type Tri = 'ignore' | 'include' | 'exclude';

/** What the Discover page searches for. */
export interface Filters {
	/** Module ID; empty for every module. */
	module: string;
	q: string;
	/** Genres not listed are ignored. */
	genres: Record<string, Tri>;
	/** Exact status (`0`–`3`); empty for any. */
	status: string;
	/** The MangaBaka format (`manga`, …, `unknown`); empty for any. */
	format: string;
	/** The MangaBaka publication status (`ongoing`, …, `unknown`); empty for any. */
	publication: string;
	/** 1-based. */
	page: number;
}

export const emptyFilters = (): Filters => ({
	module: '',
	q: '',
	genres: {},
	status: '',
	format: '',
	publication: '',
	page: 1
});

/** A click on a chip: ignore → include → exclude → ignore. */
export function cycle(state: Tri): Tri {
	return state === 'ignore' ? 'include' : state === 'include' ? 'exclude' : 'ignore';
}

const genresIn = (genres: Record<string, Tri>, state: Tri): string =>
	Object.entries(genres)
		.filter(([, s]) => s === state)
		.map(([genre]) => genre)
		.join(',');

/** The `GET /api/lists/search` query for `filters`; empty and default values are left out. */
export function searchQuery(filters: Filters): SearchQuery {
	const query: SearchQuery = { ...facetQuery(filters) };
	const include = genresIn(filters.genres, 'include');
	const exclude = genresIn(filters.genres, 'exclude');
	if (include) query.genres_include = include;
	if (exclude) query.genres_exclude = exclude;
	if (filters.status) query.status = filters.status;
	if (filters.format) query.format = filters.format;
	if (filters.publication) query.publication = filters.publication;
	if (filters.page > 1) query.page = filters.page;
	return query;
}

/** The `GET /api/lists/facets` query for `filters`: only the module and text narrow facets. */
export function facetQuery(filters: Filters): FacetQuery {
	const query: FacetQuery = {};
	const q = filters.q.trim();
	if (filters.module) query.module = filters.module;
	if (q) query.q = q;
	return query;
}
