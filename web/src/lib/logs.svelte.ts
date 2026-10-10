import type { LogLevel, LogLine } from '#lib/api/types.ts';

/** Log levels, most severe first (the server's order). */
export const LEVELS: readonly LogLevel[] = ['ERROR', 'WARN', 'INFO', 'DEBUG', 'TRACE'];

export interface LogFilter {
	/** Show lines at this level or more severe. */
	level: LogLevel;
	/** Show only lines of this website module; empty for all. */
	module: string;
	/** Case-insensitive text to find in the message, target or module; empty for all. */
	search: string;
}

/** The lines matching `filter`, in their original order. */
export function filterLogs(lines: readonly LogLine[], filter: LogFilter): LogLine[] {
	const maxRank = LEVELS.indexOf(filter.level);
	const needle = filter.search.trim().toLowerCase();
	return lines.filter(
		(l) =>
			LEVELS.indexOf(l.level) <= maxRank &&
			(!filter.module || l.module === filter.module) &&
			(!needle ||
				l.message.toLowerCase().includes(needle) ||
				l.target.toLowerCase().includes(needle) ||
				(l.module ?? '').toLowerCase().includes(needle))
	);
}

/** One line as plain text, for the clipboard. */
export function formatLine(l: LogLine): string {
	const module = l.module ? `[${l.module}] ` : '';
	return `${l.time} ${l.level.padEnd(5)} ${module}${l.target}: ${l.message}`;
}

export const MAX_LOG_LINES = 10_000;

export interface LogFeedOptions {
	/** How many lines to keep; older ones are dropped. */
	max?: number;
	/** Runs a batched flush later; defaults to the next animation frame. */
	schedule?: (flush: () => void) => void;
}

const nextFrame = (fn: () => void): void => {
	if (typeof requestAnimationFrame === 'function') requestAnimationFrame(fn);
	else setTimeout(fn, 16);
};

/**
 * The newest log lines, oldest first, fed by live `log` frames and `GET /api/logs` snapshots.
 * Live lines are batched and applied once per frame, so a burst of lines costs one update.
 */
export class LogFeed {
	lines = $state.raw<LogLine[]>([]);

	#max: number;
	#schedule: (flush: () => void) => void;
	#queue: LogLine[] = [];

	constructor({ max = MAX_LOG_LINES, schedule = nextFrame }: LogFeedOptions = {}) {
		this.#max = max;
		this.#schedule = schedule;
	}

	/** Queues a live line for the next batched flush. */
	push(line: LogLine): void {
		if (this.#queue.push(line) === 1) this.#schedule(() => this.flush());
	}

	flush(): void {
		const queued = this.#queue;
		this.#queue = [];
		this.add(queued);
	}

	/** Merges `lines` in by sequence number; ones already present are skipped. */
	add(lines: readonly LogLine[]): void {
		if (lines.length === 0) return;
		const last = this.lines.at(-1)?.seq ?? 0;
		let merged: LogLine[];
		if (lines.every((l, i) => l.seq > (i === 0 ? last : (lines[i - 1]?.seq ?? last)))) {
			merged = this.lines.concat(lines);
		} else {
			// Stable sort keeps the copy already present first, so dropping repeats keeps it.
			merged = this.lines
				.concat(lines)
				.sort((a, b) => a.seq - b.seq)
				.filter((l, i, all) => i === 0 || all[i - 1]?.seq !== l.seq);
		}
		this.lines = merged.length > this.#max ? merged.slice(-this.#max) : merged;
	}
}

/** What the Logs panel shows of a feed: filtered, and frozen while paused. */
export class LogView {
	paused = $state(false);
	filter = $state<LogFilter>({ level: 'TRACE', module: '', search: '' });

	#feed: LogFeed;
	#frozen = $state.raw<LogLine[]>([]);

	/** The lines to show, oldest first. */
	shown: LogLine[];
	/** Website modules seen in the feed, sorted. */
	modules: string[];
	/** Lines that arrived since pausing. */
	pending: number;

	constructor(feed: LogFeed) {
		this.#feed = feed;
		this.shown = $derived(filterLogs(this.paused ? this.#frozen : this.#feed.lines, this.filter));
		this.modules = $derived(
			this.#feed.lines
				.flatMap((l) => (l.module ? [l.module] : []))
				.sort()
				.filter((m, i, all) => m !== all[i - 1])
		);
		this.pending = $derived.by(() => {
			if (!this.paused) return 0;
			const last = this.#frozen.at(-1)?.seq ?? 0;
			return this.#feed.lines.filter((l) => l.seq > last).length;
		});
	}

	pause(): void {
		this.#frozen = this.#feed.lines;
		this.paused = true;
	}

	resume(): void {
		this.paused = false;
		this.#frozen = [];
	}

	/** The shown lines as plain text, one per line. */
	text(): string {
		return this.shown.map(formatLine).join('\n');
	}
}

/** Rows rendered beyond each edge of the viewport, so fast scrolling shows no gaps. */
const OVERSCAN = 10;

/** The rows `[start, end)` a virtualised list of fixed-height rows needs to render. */
export function visibleRange(v: {
	scrollTop: number;
	height: number;
	rowHeight: number;
	count: number;
}): { start: number; end: number } {
	const first = Math.floor(v.scrollTop / v.rowHeight);
	const rows = Math.ceil(v.height / v.rowHeight);
	return {
		start: Math.max(0, Math.min(first - OVERSCAN, v.count)),
		end: Math.max(0, Math.min(first + rows + OVERSCAN, v.count))
	};
}

/** Whether a scroll container is (within a few pixels of) scrolled to the bottom. */
export function atBottom(el: { scrollTop: number; clientHeight: number; scrollHeight: number }) {
	return el.scrollHeight - el.scrollTop - el.clientHeight <= 4;
}

/** The index of the first of `lines` (sorted by `seq`) with a sequence number of at least `seq`. */
export function indexAtSeq(lines: readonly LogLine[], seq: number): number {
	let lo = 0;
	let hi = lines.length;
	while (lo < hi) {
		const mid = (lo + hi) >> 1;
		if ((lines[mid]?.seq ?? Infinity) < seq) lo = mid + 1;
		else hi = mid;
	}
	return lo;
}
