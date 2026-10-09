import type { ModuleSummary } from '#lib/api/types.ts';

/** A module summary with no list, in the given category ("Raw" by default). */
export const summary = (
	id: string,
	name: string,
	root_url: string,
	category = 'Raw'
): ModuleSummary => ({
	id,
	name,
	root_url,
	category,
	option_count: 0,
	capabilities: { update_list: true, info: true, download: true, account: false },
	list_size: 0,
	list_updated: null,
	list_job_running: false
});

/** The ID upstream lua/modules/Manga1001.lua:18-19 registers two websites under. */
export const HACHIRAW_ID = '1d09f3bea8f148fa9e9215fc578fedcd';

/**
 * Upstream's two websites under one ID, both named "HachiRaw" once `Init` has run and both in
 * the "Raw" group, beside a module of its own.
 */
export const HACHIRAW: ModuleSummary[] = [
	summary(HACHIRAW_ID, 'HachiRaw', 'https://manga1001.win'),
	summary(HACHIRAW_ID, 'HachiRaw', 'https://hachiraw.win'),
	summary('other', 'Other', 'https://other.example')
];
