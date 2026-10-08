import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { InboxItem, TaskProgress } from '#lib/api/types.ts';
import { EventStore, type EventSourceLike } from '#lib/events.svelte.ts';

/** A stand-in for the browser's EventSource that the test drives by hand. */
class FakeEventSource implements EventSourceLike {
	static instances: FakeEventSource[] = [];
	onopen: ((ev: Event) => void) | null = null;
	onerror: ((ev: Event) => void) | null = null;
	closed = false;
	private listeners = new Map<string, ((ev: MessageEvent<string>) => void)[]>();

	constructor(readonly url: string) {
		FakeEventSource.instances.push(this);
	}

	addEventListener(type: string, listener: (ev: MessageEvent<string>) => void): void {
		this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
	}

	close(): void {
		this.closed = true;
	}

	open(): void {
		this.onopen?.(new Event('open'));
	}

	fail(): void {
		this.onerror?.(new Event('error'));
	}

	emit(type: string, payload: unknown): void {
		const ev = new MessageEvent<string>(type, { data: JSON.stringify(payload) });
		for (const listener of this.listeners.get(type) ?? []) listener(ev);
	}
}

const latest = (): FakeEventSource => {
	const es = FakeEventSource.instances.at(-1);
	if (!es) throw new Error('no EventSource was created');
	return es;
};

const progress = (over: Partial<TaskProgress> = {}): TaskProgress => ({
	id: 1,
	title: 'Kagurabachi',
	chapters: 'Ch. 97–98',
	status: 'downloading',
	done: 22,
	total: 38,
	bytes_per_sec: 1_800_000,
	...over
});

const inboxItem: InboxItem = {
	id: 'new-chapters',
	kind: 'warn',
	title: 'New chapters for 3 favorites',
	body: 'Kagurabachi (+2)',
	created_at: '2026-10-08T09:12:00Z',
	read: false
};

describe('event store', () => {
	beforeEach(() => {
		FakeEventSource.instances = [];
		vi.useFakeTimers();
	});
	afterEach(() => {
		vi.useRealTimers();
	});

	const start = () => {
		const store = new EventStore({
			url: '/api/events',
			connect: (url) => new FakeEventSource(url)
		});
		store.start();
		return store;
	};

	it('connects to /api/events', () => {
		start();
		expect(latest().url).toBe('/api/events');
	});

	it('tracks task progress frames by task id', () => {
		const store = start();
		latest().emit('task.progress', progress({ done: 22 }));
		latest().emit(
			'task.progress',
			progress({ id: 2, title: 'Blue Lock', status: 'queued', done: 0 })
		);
		latest().emit('task.progress', progress({ done: 23 }));

		expect(store.tasks[1]?.done).toBe(23);
		expect(store.tasks[2]?.title).toBe('Blue Lock');
		expect(Object.keys(store.tasks)).toHaveLength(2);
	});

	it('adds inbox.new frames to the inbox, newest first', () => {
		const store = start();
		latest().emit('inbox.new', inboxItem);
		latest().emit('inbox.new', {
			...inboxItem,
			id: 'module-updates',
			title: '14 module updates ready'
		});

		expect(store.inbox.map((i) => i.id)).toEqual(['module-updates', 'new-chapters']);
		expect(store.unread).toBe(2);
	});

	it('reconnects after an error, doubling the delay up to a cap', () => {
		const store = start();
		latest().open();
		expect(store.connected).toBe(true);

		const delays: number[] = [];
		for (let attempt = 0; attempt < 7; attempt++) {
			const failed = latest();
			failed.fail();
			expect(failed.closed).toBe(true);
			expect(store.connected).toBe(false);

			const before = FakeEventSource.instances.length;
			let waited = 0;
			while (FakeEventSource.instances.length === before) {
				vi.advanceTimersByTime(250);
				waited += 250;
				if (waited > 120_000) throw new Error('never reconnected');
			}
			delays.push(waited);
		}

		expect(delays).toEqual([1000, 2000, 4000, 8000, 16000, 30000, 30000]);
	});

	it('resets the backoff once a connection opens', () => {
		start();
		latest().fail();
		vi.advanceTimersByTime(1000);
		latest().fail();
		vi.advanceTimersByTime(2000);
		latest().open();

		const before = FakeEventSource.instances.length;
		latest().fail();
		vi.advanceTimersByTime(999);
		expect(FakeEventSource.instances).toHaveLength(before);
		vi.advanceTimersByTime(1);
		expect(FakeEventSource.instances).toHaveLength(before + 1);
	});

	it('keeps state from frames received on the new connection', () => {
		const store = start();
		latest().fail();
		vi.advanceTimersByTime(1000);
		latest().emit('task.progress', progress({ done: 30 }));

		expect(store.tasks[1]?.done).toBe(30);
	});

	it('stops reconnecting once stopped', () => {
		const store = start();
		latest().fail();
		store.stop();
		vi.advanceTimersByTime(60_000);

		expect(FakeEventSource.instances).toHaveLength(1);
	});

	it('tracks background job states by id', () => {
		const store = start();
		latest().emit('job.state', {
			id: 'favorites',
			title: 'Checking favorites',
			state: 'running',
			done: 31,
			total: 48
		});
		latest().emit('job.state', {
			id: 'favorites',
			title: 'Checking favorites',
			state: 'done',
			done: 48,
			total: 48
		});

		expect(store.jobs['favorites']?.state).toBe('done');
	});

	it('keeps the most recent log lines, oldest dropped first', () => {
		const store = new EventStore({
			url: '/api/events',
			connect: (url) => new FakeEventSource(url),
			maxLogLines: 3
		});
		store.start();
		for (const n of [1, 2, 3, 4]) {
			latest().emit('log', {
				seq: n,
				time: '2026-10-08T09:41:07Z',
				level: 'INFO',
				target: 'download',
				module: null,
				message: `line ${n}`
			});
		}
		vi.runOnlyPendingTimers();

		expect(store.logs.lines.map((l) => l.message)).toEqual(['line 2', 'line 3', 'line 4']);
	});

	it('merges a jobs snapshot under job frames that already arrived', () => {
		const store = start();
		const job = {
			id: 'favorites',
			title: 'Check favorites',
			state: 'idle' as const,
			done: 0,
			total: 0
		};
		latest().emit('job.state', { ...job, state: 'running', done: 3, total: 10 });
		store.seed({ jobs: [job, { ...job, id: 'lists', title: 'Update lists' }] });

		expect(store.jobs['favorites']?.state).toBe('running');
		expect(store.jobs['lists']?.state).toBe('idle');
	});

	it('merges an API snapshot under frames that already arrived', () => {
		const store = start();
		latest().emit('task.progress', progress({ done: 30 }));
		latest().emit('inbox.new', { ...inboxItem, title: 'from the stream' });

		store.seed({
			inbox: [inboxItem, { ...inboxItem, id: 'older', read: true }],
			tasks: [progress({ done: 22 }), progress({ id: 3, status: 'queued', done: 0 })]
		});

		expect(store.tasks[1]?.done).toBe(30);
		expect(store.tasks[3]?.status).toBe('queued');
		expect(store.inbox.map((i) => [i.id, i.title])).toEqual([
			['new-chapters', 'from the stream'],
			['older', inboxItem.title]
		]);
	});

	it('marks an inbox item read', () => {
		const store = start();
		latest().emit('inbox.new', inboxItem);
		store.markRead('new-chapters');

		expect(store.inbox[0]?.read).toBe(true);
		expect(store.unread).toBe(0);
	});
});
