import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { InboxItem, TaskProgress, TaskSummary } from '#lib/api/types.ts';
import { EventStore, type EventSourceLike } from '#lib/events.svelte.ts';
import { QueueStore } from '#lib/queue.svelte.ts';

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

const task = (id: number, title: string): TaskSummary => ({
	id,
	title,
	module_id: 'mangadex',
	link: `/title/${id}`,
	save_to: `/downloads/${title}`,
	status: 'waiting',
	enabled: true,
	running: false,
	error: null,
	chapters: 'Ch. 1',
	chapter_count: 1,
	chapters_done: 0,
	current_chapter: 0,
	done: 0,
	total: 0,
	bytes_per_sec: 0,
	date_added: '2026-10-01T10:00:00Z',
	date_last_downloaded: null
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

	it('routes task frames to the queue', () => {
		const store = start();
		store.queue.load([task(1, 'Kagurabachi'), task(2, 'Blue Lock')]);
		latest().emit('task.progress', progress({ done: 22 }));
		latest().emit('task.progress', progress({ done: 23 }));
		latest().emit('task.status', { id: 2, status: 'failed', error: 'HTTP 403' });

		expect(store.queue.tasks.map((t) => [t.id, t.status, t.done])).toEqual([
			[1, 'downloading', 23],
			[2, 'failed', 0]
		]);
		latest().emit('task.removed', { id: 1 });
		expect(store.queue.tasks.map((t) => t.id)).toEqual([2]);
	});

	it('refetches the queue when a connection opens, as frames may have been missed', async () => {
		const refresh = vi.fn(() => Promise.resolve([task(1, 'Kagurabachi')]));
		const store = new EventStore({
			url: '/api/events',
			connect: (url) => new FakeEventSource(url),
			queue: new QueueStore({ refresh })
		});
		store.start();
		latest().open();
		await vi.runAllTimersAsync();
		expect(refresh).toHaveBeenCalledOnce();
		expect(store.queue.tasks.map((t) => t.title)).toEqual(['Kagurabachi']);
		store.stop();
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
		store.queue.load([task(1, 'Kagurabachi')]);
		latest().fail();
		vi.advanceTimersByTime(1000);
		latest().emit('task.progress', progress({ done: 30 }));

		expect(store.queue.tasks[0]?.done).toBe(30);
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
		latest().emit('inbox.new', { ...inboxItem, title: 'from the stream' });

		store.seed({ inbox: [inboxItem, { ...inboxItem, id: 'older', read: true }] });

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
