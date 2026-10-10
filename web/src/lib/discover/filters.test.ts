import { describe, expect, it } from 'vitest';
import {
	cycle,
	emptyFilters,
	facetQuery,
	filtersFromQuery,
	queryString,
	searchQuery,
	type Filters
} from '#lib/discover/filters.ts';

const filters = (patch: Partial<Filters>): Filters => ({ ...emptyFilters(), ...patch });

describe('tri-state genre chips', () => {
	it('cycle ignore → include → exclude → ignore', () => {
		expect(cycle('ignore')).toBe('include');
		expect(cycle('include')).toBe('exclude');
		expect(cycle('exclude')).toBe('ignore');
	});
});

describe('search query', () => {
	it('is empty for no filters', () => {
		expect(searchQuery(emptyFilters())).toEqual({});
	});

	it('sends included and excluded genres comma-separated and leaves ignored ones out', () => {
		const query = searchQuery(
			filters({
				genres: { Action: 'include', Romance: 'exclude', Comedy: 'ignore', Drama: 'include' }
			})
		);
		expect(query).toEqual({ genres_include: 'Action,Drama', genres_exclude: 'Romance' });
	});

	it('sends the module, trimmed text, status and pages after the first', () => {
		const query = searchQuery(
			filters({ module: 'mangadex', q: '  one piece ', status: '1', page: 3 })
		);
		expect(query).toEqual({ module: 'mangadex', q: 'one piece', status: '1', page: 3 });
		expect(searchQuery(filters({ q: '   ', page: 1 }))).toEqual({});
	});

	it('facets only take the module and text', () => {
		const query = facetQuery(
			filters({ module: 'm', q: 'x', genres: { Action: 'include' }, status: '0', page: 2 })
		);
		expect(query).toEqual({ module: 'm', q: 'x' });
	});
});

describe('filters from a query string', () => {
	const roundTrip = (f: Filters) => filtersFromQuery(new URLSearchParams(queryString(f)));

	it('gives back filters with every field set', () => {
		const f = filters({
			module: 'mangadex',
			q: 'one piece',
			genres: { Action: 'include', Drama: 'include', Romance: 'exclude' },
			status: '2',
			format: 'manhwa',
			publication: 'unknown',
			page: 3
		});
		expect(roundTrip(f)).toEqual(f);
	});

	it('gives back empty filters', () => {
		expect(roundTrip(emptyFilters())).toEqual(emptyFilters());
	});

	it('drops unknown and malformed values', () => {
		const query = new URLSearchParams(
			'status=7&format=novel&publication=soon&genres_include=Action,,&genres_exclude=,&page=x&sort=new'
		);
		expect(filtersFromQuery(query)).toEqual(filters({ genres: { Action: 'include' } }));
		expect(filtersFromQuery(new URLSearchParams('page=0'))).toEqual(emptyFilters());
		expect(filtersFromQuery(new URLSearchParams('page=2.5'))).toEqual(emptyFilters());
	});
});
