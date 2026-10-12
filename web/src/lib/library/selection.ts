// Series picked in the Library's selection mode, by favorite id. The selection outlives the
// filters: a series a filter hides stays picked, and the bar counts it as hidden.
import type { FavoriteView } from '#lib/api/types.ts';

/** `selected` plus every series `shown` (after filters and search). */
export function selectShown(selected: ReadonlySet<number>, shown: FavoriteView[]): Set<number> {
	const next = new Set(selected);
	for (const f of shown) next.add(f.id);
	return next;
}

/** `selected` with series `id` flipped. */
export function toggle(selected: ReadonlySet<number>, id: number): Set<number> {
	const next = new Set(selected);
	if (!next.delete(id)) next.add(id);
	return next;
}

/** How many selected series the filters hide. */
export function hiddenCount(selected: ReadonlySet<number>, shown: FavoriteView[]): number {
	const visible = shown.filter((f) => selected.has(f.id)).length;
	return selected.size - visible;
}

/** "3 selected", or "3 selected (1 hidden)" when the filters hide some of them. */
export function selectionLabel(selected: ReadonlySet<number>, shown: FavoriteView[]): string {
	const hidden = hiddenCount(selected, shown);
	return `${selected.size} selected${hidden > 0 ? ` (${hidden} hidden)` : ''}`;
}

/** `selected` without the series no longer in `library`, e.g. after removing them. */
export function inLibrary(selected: ReadonlySet<number>, library: FavoriteView[]): Set<number> {
	return new Set(library.filter((f) => selected.has(f.id)).map((f) => f.id));
}
