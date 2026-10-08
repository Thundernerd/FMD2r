import type {
	FavoritesEvent,
	FavoritesEventKind,
	InboxItem,
	JobState,
	ListEvent,
	ListEventKind,
	LogLine,
	TaskProgress,
	TaskRemoved,
	TaskStatusChange
} from '#lib/api/types.ts';
import { LogFeed, MAX_LOG_LINES } from '#lib/logs.svelte.ts';
import { QueueStore } from '#lib/queue.svelte.ts';

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
	/** Where `task.*` frames go; a queue that never refetches when omitted. */
	queue?: QueueStore;
}

/** The `job.lists.<kind>` events the server sends. */
const LIST_EVENT_KINDS: ListEventKind[] = [
	'started',
	'progress',
	'finished',
	'cancelled',
	'failed'
];

/** The `job.favorites.<kind>` events the server sends. */
const FAVORITES_EVENT_KINDS: FavoritesEventKind[] = [
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
	/** The download queue, fed by `task.*` frames. */
	readonly queue: QueueStore;
	/** Inbox items, newest first. */
	inbox = $state<InboxItem[]>([]);
	unread = $derived(this.inbox.filter((i) => !i.read).length);
	jobs = $state<Record<string, JobState>>({});
	/** The latest list update or FMD2-DB import event of each module, by module ID. */
	lists = $state<Record<string, ListEvent>>({});
	/** The latest favorites check event, or `null` before the first. */
	favorites = $state<FavoritesEvent | null>(null);
	/** Recent log lines, oldest first. */
	logs: LogFeed;
	connected = $state(false);

	#opts: Required<Omit<EventStoreOptions, 'queue'>>;
	#source: EventSourceLike | null = null;
	#retry: ReturnType<typeof setTimeout> | null = null;
	#backoff = INITIAL_BACKOFF_MS;
	/** `job.state` frames received per job, to tell whether an API answer is still current. */
	#jobFrames: Record<string, number> = {};

	constructor(opts: EventStoreOptions) {
		const { queue, ...rest } = opts;
		this.#opts = { maxLogLines: MAX_LOG_LINES, ...rest };
		this.queue = queue ?? new QueueStore();
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
	seed(snapshot: { inbox?: InboxItem[]; jobs?: JobState[] }): void {
		const fresh = (snapshot.inbox ?? []).filter((s) => !this.inbox.some((i) => i.id === s.id));
		this.inbox = [...this.inbox, ...fresh];
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
			// Task frames sent while disconnected are lost; catch up from the API.
			this.queue.resync();
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
			this.queue.progress(JSON.parse(ev.data) as TaskProgress);
		});
		es.addEventListener('task.status', (ev) => {
			this.queue.status(JSON.parse(ev.data) as TaskStatusChange);
		});
		es.addEventListener('task.removed', (ev) => {
			this.queue.removed((JSON.parse(ev.data) as TaskRemoved).id);
		});
		es.addEventListener('task.reordered', () => {
			this.queue.reordered();
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
		for (const kind of FAVORITES_EVENT_KINDS) {
			es.addEventListener(`job.favorites.${kind}`, (ev) => {
				this.favorites = JSON.parse(ev.data) as FavoritesEvent;
			});
		}
		es.addEventListener('log', (ev) => {
			this.logs.push(JSON.parse(ev.data) as LogLine);
		});
	}
}
