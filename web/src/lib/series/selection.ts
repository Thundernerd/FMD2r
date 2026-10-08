// Chapter selection on the Series page. Chapters are identified by their index in module order
// (the order the module lists them in); the page may display them reversed.

/** Indices of `count` chapters in the order they are displayed. */
export function displayOrder(count: number, reversed: boolean): number[] {
	const order = Array.from({ length: count }, (_, i) => i);
	return reversed ? order.reverse() : order;
}

export function selectAll(count: number): Set<number> {
	return new Set(displayOrder(count, false));
}

/** The chapters not downloaded yet. */
export function selectNew(chapters: readonly { downloaded: boolean }[]): Set<number> {
	const selected = new Set<number>();
	chapters.forEach((c, i) => {
		if (!c.downloaded) selected.add(i);
	});
	return selected;
}

/** `selected` with chapter `index` flipped. */
export function toggle(selected: ReadonlySet<number>, index: number): Set<number> {
	const next = new Set(selected);
	if (!next.delete(index)) next.add(index);
	return next;
}

/** `selected` plus every chapter displayed (per `order`) from `anchor` to `target`, inclusive. */
export function extendRange(
	selected: ReadonlySet<number>,
	order: readonly number[],
	anchor: number,
	target: number
): Set<number> {
	const next = new Set(selected);
	const a = order.indexOf(anchor);
	const b = order.indexOf(target);
	if (a < 0 || b < 0) return next;
	for (let i = Math.min(a, b); i <= Math.max(a, b); i++) next.add(order[i] as number);
	return next;
}

/**
 * The chapters `text` names as comma-separated numbers and `from-to` ranges, counted from 1 in
 * module order; `null` when any part is malformed or out of `1..count`.
 */
export function parseRanges(text: string, count: number): Set<number> | null {
	const parts = text.split(',').map((p) => p.trim());
	if (parts.every((p) => p === '')) return null;
	const selected = new Set<number>();
	for (const part of parts) {
		if (part === '') continue;
		const m = /^(\d+)(?:\s*-\s*(\d+))?$/.exec(part);
		if (!m) return null;
		const from = Number(m[1]);
		const to = m[2] === undefined ? from : Number(m[2]);
		const [lo, hi] = from <= to ? [from, to] : [to, from];
		if (lo < 1 || hi > count) return null;
		for (let n = lo; n <= hi; n++) selected.add(n - 1);
	}
	return selected;
}
