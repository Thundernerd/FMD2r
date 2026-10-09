import type { ModuleSummary } from '#lib/api/types.ts';

/**
 * A key for a module in a list over `GET /api/modules`. IDs alone can repeat: FMD2 keeps every
 * module a file's `Init` creates without checking IDs (baseunits/lua/LuaWebsiteModules.pas:523-589),
 * and upstream lua/modules/Manga1001.lua:18-19 registers two websites under one ID.
 */
export const moduleKey = (m: ModuleSummary): string => `${m.id}\n${m.root_url}`;

/** The host of a module's root URL, or the root URL itself when it doesn't parse. */
export function moduleHost(m: ModuleSummary): string {
	try {
		return new URL(m.root_url).host || m.root_url;
	} catch {
		return m.root_url;
	}
}

/** The names more than one of `modules` share, whose entries the pickers tell apart by host. */
export function repeatedNames(modules: ModuleSummary[]): Set<string> {
	const seen = new Set<string>();
	const repeated = new Set<string>();
	for (const m of modules) (seen.has(m.name) ? repeated : seen).add(m.name);
	return repeated;
}

/** How `m` is listed among `repeated` names: by name, with its host when another module has that name too. */
export const moduleLabel = (m: ModuleSummary, repeated: Set<string>): string =>
	repeated.has(m.name) ? `${m.name} (${moduleHost(m)})` : m.name;

export interface ModuleGroup {
	category: string;
	modules: ModuleSummary[];
}

/** The category a module is listed under; modules without one are listed under "Other". */
export const moduleCategory = (m: ModuleSummary): string => m.category || 'Other';

const compareIgnoringCase = (a: string, b: string) =>
	a.localeCompare(b, undefined, { sensitivity: 'base' });

/**
 * `modules` grouped by category, the groups sorted by category and each group's modules by name,
 * ignoring case; modules sharing a name are sorted by host.
 */
export function groupModules(modules: ModuleSummary[]): ModuleGroup[] {
	return Object.entries(Object.groupBy(modules, moduleCategory))
		.sort(([a], [b]) => compareIgnoringCase(a, b))
		.map(([category, list = []]) => ({
			category,
			modules: list.toSorted(
				(a, b) =>
					compareIgnoringCase(a.name, b.name) || compareIgnoringCase(moduleHost(a), moduleHost(b))
			)
		}));
}

/** Whether `text` contains every word of the search `query`, ignoring case. */
export function matchesSearch(text: string, query: string): boolean {
	const lower = text.toLowerCase();
	return query
		.toLowerCase()
		.split(/\s+/)
		.filter(Boolean)
		.every((w) => lower.includes(w));
}
