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
