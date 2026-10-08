import { describe, expect, it } from 'vitest';
import type { LogLine } from '#lib/api/types.ts';
import {
	LogFeed,
	LogView,
	atBottom,
	filterLogs,
	formatLine,
	visibleRange
} from '#lib/logs.svelte.ts';

let nextSeq = 1;
const line = (over: Partial<LogLine> = {}): LogLine => ({
	seq: nextSeq++,
	time: '2026-10-08T09:41:07Z',
	level: 'INFO',
	target: 'fmd.logger',
	module: null,
	message: 'hello',
	...over
});

describe('log filtering', () => {
	const lines = [
		line({ level: 'INFO', module: 'MangaDex', message: 'chapter list loaded' }),
		line({ level: 'WARN', module: 'MangaDex', message: 'rate limited' }),
		line({ level: 'ERROR', module: 'Bato.to', message: 'no pages' }),
		line({ level: 'DEBUG', target: 'fmd_server', message: 'request served' }),
		line({ level: 'WARN', target: 'fmd_server', message: 'Slow request' })
	];
	const messages = (ls: LogLine[]) => ls.map((l) => l.message);

	it('keeps lines at the chosen level or more severe', () => {
		const shown = filterLogs(lines, { level: 'WARN', module: '', search: '' });
		expect(messages(shown)).toEqual(['rate limited', 'no pages', 'Slow request']);
	});

	it('shows every level at TRACE', () => {
		expect(filterLogs(lines, { level: 'TRACE', module: '', search: '' })).toHaveLength(5);
	});

	it('keeps only lines of the chosen module', () => {
		const shown = filterLogs(lines, { level: 'TRACE', module: 'MangaDex', search: '' });
		expect(messages(shown)).toEqual(['chapter list loaded', 'rate limited']);
	});

	it('searches message, target and module, ignoring case', () => {
		const search = (q: string) =>
			messages(filterLogs(lines, { level: 'TRACE', module: '', search: q }));
		expect(search('slow')).toEqual(['Slow request']);
		expect(search('FMD_SERVER')).toEqual(['request served', 'Slow request']);
		expect(search('bato')).toEqual(['no pages']);
	});

	it('combines all filters', () => {
		const shown = filterLogs(lines, { level: 'WARN', module: 'MangaDex', search: 'rate' });
		expect(messages(shown)).toEqual(['rate limited']);
	});

	it('formats a line for copying', () => {
		expect(formatLine(lines[1]!)).toBe(
			'2026-10-08T09:41:07Z WARN  [MangaDex] fmd.logger: rate limited'
		);
		expect(formatLine(lines[3]!)).toBe('2026-10-08T09:41:07Z DEBUG fmd_server: request served');
	});
});

describe('log feed', () => {
	/** A feed whose batched flushes the test triggers by hand. */
	const manualFeed = (max = 10_000) => {
		const flushes: (() => void)[] = [];
		const feed = new LogFeed({ max, schedule: (fn) => flushes.push(fn) });
		return { feed, flush: () => flushes.splice(0).forEach((fn) => fn()) };
	};

	it('batches live lines until the next flush', () => {
		const { feed, flush } = manualFeed();
		feed.push(line({ message: 'a' }));
		feed.push(line({ message: 'b' }));
		expect(feed.lines).toEqual([]);
		flush();
		expect(feed.lines.map((l) => l.message)).toEqual(['a', 'b']);
	});

	it('keeps only the newest lines', () => {
		const { feed, flush } = manualFeed(3);
		for (const n of [1, 2, 3, 4, 5]) feed.push(line({ message: `line ${n}` }));
		flush();
		expect(feed.lines.map((l) => l.message)).toEqual(['line 3', 'line 4', 'line 5']);
	});

	it('merges an older snapshot under live lines, without duplicates', () => {
		const { feed, flush } = manualFeed();
		const [a, b, c] = [line({ message: 'a' }), line({ message: 'b' }), line({ message: 'c' })];
		feed.push(c);
		flush();
		feed.add([a, b, c]);
		expect(feed.lines.map((l) => l.message)).toEqual(['a', 'b', 'c']);
	});

	it('handles 10k lines arriving one by one in a single flush', () => {
		const { feed, flush } = manualFeed();
		for (let n = 0; n < 12_000; n++) feed.push(line({ message: `line ${n}` }));
		flush();
		expect(feed.lines).toHaveLength(10_000);
		expect(feed.lines[0]?.message).toBe('line 2000');
		expect(feed.lines.at(-1)?.message).toBe('line 11999');
	});
});

describe('log view', () => {
	const setup = () => {
		const feed = new LogFeed({ schedule: () => {} });
		const view = new LogView(feed);
		return { feed, view };
	};
	const messages = (view: LogView) => view.shown.map((l) => l.message);

	it('shows new lines while live and freezes while paused', () => {
		const { feed, view } = setup();
		feed.add([line({ message: 'a' })]);
		expect(messages(view)).toEqual(['a']);

		view.pause();
		feed.add([line({ message: 'b' }), line({ message: 'c' })]);
		expect(messages(view)).toEqual(['a']);
		expect(view.pending).toBe(2);

		view.resume();
		expect(messages(view)).toEqual(['a', 'b', 'c']);
		expect(view.pending).toBe(0);
	});

	it('applies its filter and lists the modules seen', () => {
		const { feed, view } = setup();
		feed.add([
			line({ module: 'MangaDex', message: 'a' }),
			line({ module: 'Bato.to', level: 'ERROR', message: 'b' }),
			line({ module: 'MangaDex', message: 'c' }),
			line({ message: 'd' })
		]);
		view.filter.module = 'MangaDex';
		expect(messages(view)).toEqual(['a', 'c']);
		view.filter = { level: 'ERROR', module: '', search: '' };
		expect(messages(view)).toEqual(['b']);
		expect(view.modules).toEqual(['Bato.to', 'MangaDex']);
	});

	it('copies the shown lines as text', () => {
		const { feed, view } = setup();
		feed.add([line({ level: 'WARN', message: 'a' }), line({ level: 'INFO', message: 'b' })]);
		view.filter.level = 'WARN';
		expect(view.text()).toBe('2026-10-08T09:41:07Z WARN  fmd.logger: a');
	});
});

describe('log list scrolling', () => {
	it('renders only the rows in view plus some overscan', () => {
		// 20px rows, 200px viewport, scrolled to row 100 of 10k.
		expect(visibleRange({ scrollTop: 2000, height: 200, rowHeight: 20, count: 10_000 })).toEqual({
			start: 90,
			end: 120
		});
	});

	it('clamps the range to the list', () => {
		expect(visibleRange({ scrollTop: 0, height: 200, rowHeight: 20, count: 5 })).toEqual({
			start: 0,
			end: 5
		});
		expect(visibleRange({ scrollTop: 199_800, height: 200, rowHeight: 20, count: 10_000 })).toEqual(
			{ start: 9980, end: 10_000 }
		);
	});

	it('counts as following only when scrolled to the bottom', () => {
		expect(atBottom({ scrollTop: 800, clientHeight: 200, scrollHeight: 1000 })).toBe(true);
		expect(atBottom({ scrollTop: 798, clientHeight: 200, scrollHeight: 1000 })).toBe(true);
		expect(atBottom({ scrollTop: 700, clientHeight: 200, scrollHeight: 1000 })).toBe(false);
	});
});
