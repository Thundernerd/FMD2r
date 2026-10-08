import type { InboxItem, JobState, LogLine, TaskProgress } from '#lib/api/types.ts';

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
	/** First reconnect delay; doubles on every failed attempt. */
	initialBackoffMs?: number;
	maxBackoffMs?: number;
	/** How many log lines to keep; older ones are dropped. */
	maxLogLines?: number;
}

/** Live state fed by the server's SSE stream (`/api/events`). */
export class EventStore {
	tasks = $state<Record<number, TaskProgress>>({});
	/** Inbox items, newest first. */
	inbox = $state<InboxItem[]>([]);
	unread = $derived(this.inbox.filter((i) => !i.read).length);
	jobs = $state<Record<string, JobState>>({});
	/** Recent log lines, oldest first. */
	logs = $state<LogLine[]>([]);
	connected = $state(false);

	#opts: Required<EventStoreOptions>;
	#source: EventSourceLike | null = null;
	#retry: ReturnType<typeof setTimeout> | null = null;
	#backoff: number;

	constructor(opts: EventStoreOptions) {
		this.#opts = { initialBackoffMs: 1000, maxBackoffMs: 30_000, maxLogLines: 500, ...opts };
		this.#backoff = this.#opts.initialBackoffMs;
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

	#open(): void {
		const es = this.#opts.connect(this.#opts.url);
		this.#source = es;
		es.onopen = () => {
			this.connected = true;
			this.#backoff = this.#opts.initialBackoffMs;
		};
		// EventSource retries on its own for some failures but gives up for others (e.g. a 5xx),
		// so always close it and reconnect on our own schedule.
		es.onerror = () => {
			es.close();
			this.#source = null;
			this.connected = false;
			const delay = this.#backoff;
			this.#backoff = Math.min(this.#backoff * 2, this.#opts.maxBackoffMs);
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
			this.jobs[job.id] = job;
		});
		es.addEventListener('log', (ev) => {
			const line = JSON.parse(ev.data) as LogLine;
			this.logs = [...this.logs, line].slice(-this.#opts.maxLogLines);
		});
	}
}

export function createEventStore(opts: EventStoreOptions): EventStore {
	return new EventStore(opts);
}
