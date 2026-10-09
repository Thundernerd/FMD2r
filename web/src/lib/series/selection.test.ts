import { describe, expect, it } from 'vitest';
import {
	displayOrder,
	extendRange,
	parseRanges,
	selectAll,
	selectNew,
	toggle
} from '#lib/series/selection.ts';

// Chapters are identified by their index in module order; the list may show them reversed.
const chapters = [
	{ downloaded: true },
	{ downloaded: false },
	{ downloaded: true },
	{ downloaded: false },
	{ downloaded: false }
];
const sorted = (s: Set<number>) => [...s].sort((a, b) => a - b);

describe('chapter selection', () => {
	it('selects all, or only the chapters not seen yet', () => {
		expect(sorted(selectAll(5))).toEqual([0, 1, 2, 3, 4]);
		expect(sorted(selectNew(chapters))).toEqual([1, 3, 4]);
	});

	it('toggles one chapter without touching the others', () => {
		const once = toggle(new Set([1]), 3);
		expect(sorted(once)).toEqual([1, 3]);
		expect(sorted(toggle(once, 1))).toEqual([3]);
	});

	it('shows chapters in module order or reversed', () => {
		expect(displayOrder(4, false)).toEqual([0, 1, 2, 3]);
		expect(displayOrder(4, true)).toEqual([3, 2, 1, 0]);
	});

	it('extends a range between two chapters as they are displayed, keeping the rest', () => {
		const order = displayOrder(5, true); // 4 3 2 1 0
		expect(sorted(extendRange(new Set([0]), order, 4, 2))).toEqual([0, 2, 3, 4]);
		// Either direction.
		expect(sorted(extendRange(new Set(), order, 1, 3))).toEqual([1, 2, 3]);
	});

	it('parses ranges of chapter numbers, counted from 1 in module order', () => {
		expect(sorted(parseRanges('2-4', 5) ?? new Set())).toEqual([1, 2, 3]);
		expect(sorted(parseRanges(' 1 , 3-3,5 ', 5) ?? new Set())).toEqual([0, 2, 4]);
		// A reversed range reads the same.
		expect(sorted(parseRanges('4-2', 5) ?? new Set())).toEqual([1, 2, 3]);
		// Out of range or malformed input is rejected rather than half applied.
		expect(parseRanges('0-2', 5)).toBeNull();
		expect(parseRanges('4-6', 5)).toBeNull();
		expect(parseRanges('1-x', 5)).toBeNull();
		expect(parseRanges('', 5)).toBeNull();
	});
});
