import type {
	TaskCounts,
	TaskGroup,
	TaskProgress,
	TaskState,
	TaskStatusChange,
	TaskSummary
} from '#lib/api/types.ts';

/** The Queue page's groups, in the order it shows them. */
export const GROUPS: { key: TaskGroup; label: string }[] = [
	{ key: 'downloading', label: 'Downloading' },
	{ key: 'waiting', label: 'Waiting' },
	{ key: 'stopped', label: 'Stopped / failed' },
	{ key: 'finished', label: 'Finished' }
];

/** The group a status belongs to, as fmd-server counts them. */
export function groupOf(status: TaskState): TaskGroup {
	switch (status) {
		case 'preparing':
		case 'downloading':
		case 'converting':
		case 'compressing':
			return 'downloading';
		case 'waiting':
			return 'waiting';
		case 'finished':
			return 'finished';
		default:
			return 'stopped';
	}
}

export interface QueueFilter {
	/** Matched against the title, module and chapter, ignoring case. */
	text: string;
	status: TaskGroup | 'all';
	/** `YYYY-MM-DD` in local time, inclusive; empty for no bound. */
	from: string;
	to: string;
}

export interface QueueStoreOptions {
	/** Fetches the whole queue; called when frames mention tasks the store has not seen. */
	refresh?: () => Promise<TaskSummary[]>;
	/** Speed samples kept for the graph. */
	historySize?: number;
}

/** Five minutes of one sample a second. */
const HISTORY_SIZE = 300;
/** How long frames are collected before one refetch covers them all. */
const REFRESH_DELAY_MS = 300;

/** When a task was last active: its last download, or when it was added. */
const lastActive = (task: TaskSummary) => Date.parse(task.date_last_downloaded ?? task.date_added);

/** The download queue, kept current by `task.*` frames; the Queue page and the dock show it. */
export class QueueStore {
	/** In queue order. */
	tasks = $state<TaskSummary[]>([]);
	filter = $state<QueueFilter>({ text: '', status: 'all', from: '', to: '' });
	/** Total download speed samples, oldest first. */
	history = $state<number[]>([]);

	active = $derived(this.tasks.filter((t) => groupOf(t.status) === 'downloading'));
	waiting = $derived(this.tasks.filter((t) => t.status === 'waiting').length);
	/** Bytes per second over every downloading task. */
	rate = $derived(this.active.reduce((sum, t) => sum + t.bytes_per_sec, 0));
	/** Tasks matching the text and date filters. */
	matching = $derived(this.tasks.filter((t) => this.#matches(t)));
	counts = $derived.by(() => {
		const counts: TaskCounts = { downloading: 0, waiting: 0, stopped: 0, finished: 0 };
		for (const task of this.matching) counts[groupOf(task.status)]++;
		return counts;
	});
	groups = $derived(
		GROUPS.filter((g) => this.filter.status === 'all' || this.filter.status === g.key).map((g) => ({
			...g,
			tasks: this.matching.filter((t) => groupOf(t.status) === g.key)
		}))
	);

	#refresh: QueueStoreOptions['refresh'];
	#historySize: number;
	#timer: ReturnType<typeof setTimeout> | null = null;
	/** Changes frames made while a refetch is in flight, re-applied over its (older) answer. */
	#inFlight: Record<number, Partial<TaskSummary> | null> | null = null;

	constructor({ refresh, historySize = HISTORY_SIZE }: QueueStoreOptions = {}) {
		this.#refresh = refresh;
		this.#historySize = historySize;
	}

	load(tasks: TaskSummary[]): void {
		this.tasks = tasks;
	}

	/** Adds a task or replaces it with a newer copy (e.g. an action's answer). */
	upsert(task: TaskSummary): void {
		const i = this.tasks.findIndex((t) => t.id === task.id);
		if (i < 0) this.tasks.push(task);
		else this.tasks[i] = task;
		// Newer than a refetch already in flight.
		if (this.#inFlight) this.#inFlight[task.id] = task;
	}

	progress(p: TaskProgress): void {
		const { id, chapters, status, done, total, bytes_per_sec } = p;
		this.#patch(id, { chapters, status, done, total, bytes_per_sec });
	}

	/** A `task.status` frame; a refetch follows to pick up chapter counts and dates. */
	status(s: TaskStatusChange): void {
		const patch: Partial<TaskSummary> = { status: s.status, error: s.error ?? null };
		if (groupOf(s.status) !== 'downloading') patch.bytes_per_sec = 0;
		this.#patch(s.id, patch);
		this.resync();
	}

	removed(id: number): void {
		this.tasks = this.tasks.filter((t) => t.id !== id);
		if (this.#inFlight) this.#inFlight[id] = null;
	}

	reordered(): void {
		this.resync();
	}

	/** Records the current total speed for the graph. */
	sample(): void {
		this.history = [...this.history, this.rate].slice(-this.#historySize);
	}

	/** Refetches the whole queue soon, once for any number of calls in a short while. */
	resync(): void {
		if (!this.#refresh || this.#timer !== null) return;
		const refresh = this.#refresh;
		this.#timer = setTimeout(() => {
			this.#timer = null;
			const changes: Record<number, Partial<TaskSummary> | null> = {};
			this.#inFlight = changes;
			refresh()
				.then((tasks) => {
					this.tasks = tasks.flatMap((t) => {
						if (!(t.id in changes)) return [t];
						const change = changes[t.id];
						return change ? [{ ...t, ...change }] : [];
					});
				})
				.catch(() => {})
				.finally(() => {
					if (this.#inFlight === changes) this.#inFlight = null;
				});
		}, REFRESH_DELAY_MS);
	}

	#patch(id: number, patch: Partial<TaskSummary>): void {
		const task = this.tasks.find((t) => t.id === id);
		if (task) Object.assign(task, patch);
		else this.resync();
		// A task deleted meanwhile stays deleted.
		if (this.#inFlight && this.#inFlight[id] !== null) {
			this.#inFlight[id] = { ...this.#inFlight[id], ...patch };
		}
	}

	#matches(task: TaskSummary): boolean {
		const { text, from, to } = this.filter;
		const needle = text.trim().toLowerCase();
		if (
			needle &&
			![task.title, task.module_id, task.chapters].some((s) => s.toLowerCase().includes(needle))
		) {
			return false;
		}
		const at = lastActive(task);
		if (from && at < Date.parse(`${from}T00:00:00`)) return false;
		if (to && at > Date.parse(`${to}T23:59:59.999`)) return false;
		return true;
	}
}
