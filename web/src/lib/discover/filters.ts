import type { paths } from '#lib/api/schema.d.ts';

export type SearchQuery = NonNullable<paths['/api/lists/search']['get']['parameters']['query']>;
export type FacetQuery = NonNullable<paths['/api/lists/facets']['get']['parameters']['query']>;

/**
 * A genre chip's state, as FMD2's filter genre check boxes have it: checked (must occur),
 * unchecked (must not occur) or grayed (ignored).
 */
export type Tri = 'ignore' | 'include' | 'exclude';

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

/** `MangaInfo_Status*` (baseunits/uBaseUnit.pas:230-233). */
export const STATUS: Record<string, string> = {
	'0': 'Completed',
	'1': 'Ongoing',
	'2': 'Hiatus',
	'3': 'Cancelled'
};
/** MangaBaka's formats, as the `format` filter names them. */
export const FORMAT: Record<string, string> = {
	manga: 'Manga',
	manhwa: 'Manhwa',
	manhua: 'Manhua',
	oel: 'OEL',
	other: 'Other'
};
/** MangaBaka's publication statuses, as the `publication` filter names them. */
export const PUBLICATION: Record<string, string> = {
	ongoing: 'Ongoing',
	completed: 'Completed',
	hiatus: 'Hiatus',
	cancelled: 'Cancelled'
};
/** The `format` and `publication` of the titles MangaBaka knows none for. */
export const UNKNOWN = 'unknown';

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

/** The names in a comma-separated genre list, without empty ones. */
const genreList = (value: string | null): string[] =>
	(value ?? '').split(',').filter((genre) => genre !== '');

/** `value` when `labels` names it or it is `extra`; else empty. */
const known = (value: string | null, labels: Record<string, string>, extra?: string): string =>
	value !== null && (Object.hasOwn(labels, value) || value === extra) ? value : '';

/**
 * The filters in a query string written by {@link queryString}, such as Discover's URL.
 * Unknown and malformed values are dropped.
 */
export function filtersFromQuery(query: Pick<URLSearchParams, 'get'>): Filters {
	const filters = emptyFilters();
	filters.module = query.get('module') ?? '';
	filters.q = query.get('q') ?? '';
	for (const genre of genreList(query.get('genres_include'))) filters.genres[genre] = 'include';
	for (const genre of genreList(query.get('genres_exclude'))) filters.genres[genre] = 'exclude';
	filters.status = known(query.get('status'), STATUS);
	filters.format = known(query.get('format'), FORMAT, UNKNOWN);
	filters.publication = known(query.get('publication'), PUBLICATION, UNKNOWN);
	const page = query.get('page');
	if (page !== null && /^[1-9][0-9]*$/.test(page)) filters.page = Number(page);
	return filters;
}

/** {@link searchQuery} as a query string (without `?`); empty for no filters. */
export function queryString(filters: Filters): string {
	const query = Object.entries(searchQuery(filters)).map(([key, value]) => [key, String(value)]);
	return new URLSearchParams(query).toString();
}
