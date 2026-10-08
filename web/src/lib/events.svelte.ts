import type {
	InboxItem,
	JobState,
	ListEvent,
	ListEventKind,
	LogLine,
	TaskProgress
} from '#lib/api/types.ts';
import { LogFeed, MAX_LOG_LINES } from '#lib/logs.svelte.ts';

/** The part of the browser's `EventSource` the store uses, so tests and mock mode can supply their own. */
export interface EventSourceLike {
	onopen: ((ev: Event) => void) | null;
	onerror: ((ev: Event) => void) | null;
	addEventListener(type: string, listener: (ev: MessageEvent<string>) => void): void;
	close(): void;
}

export interface EventStoreOptions {
	url: string;
	connect: (url: string) => EventSourceLike;
	/** How many log lines to keep; older ones are dropped. */
	maxLogLines?: number;
}

/** The `job.lists.<kind>` events the server sends. */
const LIST_EVENT_KINDS: ListEventKind[] = [
	'started',
	'progress',
	'finished',
	'cancelled',
	'failed'
];

/** First reconnect delay; it doubles on every failed attempt up to the cap. */
const INITIAL_BACKOFF_MS = 1000;
const MAX_BACKOFF_MS = 30_000;

/** Live state fed by the server's SSE stream (`/api/events`). */
export class EventStore {
	tasks = $state<Record<number, TaskProgress>>({});
	/** Inbox items, newest first. */
	inbox = $state<InboxItem[]>([]);
	unread = $derived(this.inbox.filter((i) => !i.read).length);
	jobs = $state<Record<string, JobState>>({});
	/** The latest list update or FMD2-DB import event of each module, by module ID. */
	lists = $state<Record<string, ListEvent>>({});
	/** Recent log lines, oldest first. */
	logs: LogFeed;
	connected = $state(false);

	#opts: Required<EventStoreOptions>;
	#source: EventSourceLike | null = null;
	#retry: ReturnType<typeof setTimeout> | null = null;
	#backoff = INITIAL_BACKOFF_MS;
	/** `job.state` frames received per job, to tell whether an API answer is still current. */
	#jobFrames: Record<string, number> = {};

	constructor(opts: EventStoreOptions) {
		this.#opts = { maxLogLines: MAX_LOG_LINES, ...opts };
		this.logs = new LogFeed({ max: this.#opts.maxLogLines });
	}

	start(): void {
		this.stop();
		this.#open();
	}

	stop(): void {
		if (this.#retry !== null) clearTimeout(this.#retry);
		this.#retry = null;
		this.#source?.close();
		this.#source = null;
		this.connected = false;
	}

	/**
	 * Merges a REST snapshot taken around connect time. Anything a frame already delivered is
	 * newer than the snapshot, so it wins.
	 */
	seed(snapshot: { inbox?: InboxItem[]; tasks?: TaskProgress[]; jobs?: JobState[] }): void {
		const fresh = (snapshot.inbox ?? []).filter((s) => !this.inbox.some((i) => i.id === s.id));
		this.inbox = [...this.inbox, ...fresh];
		for (const task of snapshot.tasks ?? []) this.tasks[task.id] ??= task;
		for (const job of snapshot.jobs ?? []) this.jobs[job.id] ??= job;
	}

	/** Marks the current state of job `id`; pass it to {@link updateJob} with a later API answer. */
	jobVersion(id: string): number {
		return this.#jobFrames[id] ?? 0;
	}

	/**
	 * Records a job state the API answered with (e.g. after starting the job), unless a
	 * `job.state` frame arrived since `version` was taken: that frame is newer.
	 */
	updateJob(job: JobState, version: number): void {
		if (this.jobVersion(job.id) === version) this.jobs[job.id] = job;
	}

	markRead(id: string): void {
		const item = this.inbox.find((i) => i.id === id);
		if (item) item.read = true;
	}

	#open(): void {
		const es = this.#opts.connect(this.#opts.url);
		this.#source = es;
		es.onopen = () => {
			this.connected = true;
			this.#backoff = INITIAL_BACKOFF_MS;
		};
		// EventSource retries on its own for some failures but gives up for others (e.g. a 5xx),
		// so always close it and reconnect on our own schedule.
		es.onerror = () => {
			es.close();
			this.#source = null;
			this.connected = false;
			const delay = this.#backoff;
			this.#backoff = Math.min(this.#backoff * 2, MAX_BACKOFF_MS);
			this.#retry = setTimeout(() => {
				this.#retry = null;
				this.#open();
			}, delay);
		};
		es.addEventListener('task.progress', (ev) => {
			const task = JSON.parse(ev.data) as TaskProgress;
			this.tasks[task.id] = task;
		});
		es.addEventListener('inbox.new', (ev) => {
			const item = JSON.parse(ev.data) as InboxItem;
			this.inbox = [item, ...this.inbox.filter((i) => i.id !== item.id)];
		});
		es.addEventListener('job.state', (ev) => {
			const job = JSON.parse(ev.data) as JobState;
			this.#jobFrames[job.id] = this.jobVersion(job.id) + 1;
			this.jobs[job.id] = job;
		});
		for (const kind of LIST_EVENT_KINDS) {
			es.addEventListener(`job.lists.${kind}`, (ev) => {
				const event = JSON.parse(ev.data) as ListEvent;
				this.lists[event.module_id] = event;
			});
		}
		es.addEventListener('log', (ev) => {
			this.logs.push(JSON.parse(ev.data) as LogLine);
		});
	}
}
