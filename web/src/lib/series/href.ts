import type { SeriesRef } from '#lib/api/types.ts';

/** The Series page of `series`. Links go in the query: they hold slashes and query strings. */
export function seriesHref(series: SeriesRef): string {
	return `/series?${new URLSearchParams({ module: series.module_id, link: series.link })}`;
}
