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

/**
 * The modules matching every word of `search` (in name or category), or for which `keep` holds,
 * grouped by category, groups and modules sorted by label.
 */
export function groupModules(
	modules: ModuleSummary[],
	search: string,
	keep: (m: ModuleSummary) => boolean = () => false
): { category: string; modules: ModuleSummary[] }[] {
	const repeated = repeatedNames(modules);
	const label = (m: ModuleSummary) => moduleLabel(m, repeated);
	const words = search.toLowerCase().split(/\s+/).filter(Boolean);
	const shown = modules.filter((m) => {
		const text = `${m.name} ${m.category}`.toLowerCase();
		return keep(m) || words.every((w) => text.includes(w));
	});
	const byCategory = Object.groupBy(shown, (m) => m.category || 'Other');
	return Object.entries(byCategory)
		.sort(([a], [b]) => a.localeCompare(b))
		.map(([category, list = []]) => ({
			category,
			modules: list.toSorted((a, b) => label(a).localeCompare(label(b)))
		}));
}
