import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { TaskProgress, TaskSummary } from '#lib/api/types.ts';
import { QueueStore } from '#lib/queue.svelte.ts';

const task = (over: Partial<TaskSummary> & Pick<TaskSummary, 'id' | 'title'>): TaskSummary => ({
	module_id: 'mangadex',
	link: `/title/${over.id}`,
	save_to: `/downloads/${over.title}`,
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
	date_last_downloaded: null,
	...over
});

/** One task per status FMD2 has, in queue order. */
const queue = (): TaskSummary[] => [
	task({ id: 1, title: 'Kagurabachi', status: 'downloading', bytes_per_sec: 1_500_000 }),
	task({ id: 2, title: 'Omniscient Reader', status: 'compressing', bytes_per_sec: 500_000 }),
	task({ id: 3, title: 'Blue Lock', status: 'waiting' }),
	task({ id: 4, title: 'Apothecary Diaries', status: 'failed', error: 'HTTP 403' }),
	task({ id: 5, title: 'Vagabond', status: 'disabled', enabled: false }),
	task({
		id: 6,
		title: 'Blame!',
		status: 'finished',
		date_added: '2026-09-01T10:00:00Z',
		date_last_downloaded: '2026-10-05T12:00:00Z'
	})
];

const progress = (over: Partial<TaskProgress> & Pick<TaskProgress, 'id'>): TaskProgress => ({
	title: 'Kagurabachi',
	chapters: 'Ch. 98 (2/2)',
	status: 'downloading',
	done: 10,
	total: 20,
	bytes_per_sec: 2_000_000,
	...over
});

const titles = (tasks: TaskSummary[]) => tasks.map((t) => t.title);

describe('queue store', () => {
	beforeEach(() => vi.useFakeTimers());
	afterEach(() => vi.useRealTimers());

	it('groups tasks by status in queue order, with counts', () => {
		const store = new QueueStore();
		store.load(queue());
		expect(store.groups.map((g) => [g.label, titles(g.tasks)])).toEqual([
			['Downloading', ['Kagurabachi', 'Omniscient Reader']],
			['Waiting', ['Blue Lock']],
			['Stopped / failed', ['Apothecary Diaries', 'Vagabond']],
			['Finished', ['Blame!']]
		]);
		expect(store.counts).toEqual({ downloading: 2, waiting: 1, stopped: 2, finished: 1 });
	});

	it('filters by text, status group and date range', () => {
		const store = new QueueStore();
		store.load(queue());

		store.filter.text = 'BLUE';
		expect(store.groups.flatMap((g) => titles(g.tasks))).toEqual(['Blue Lock']);
		expect(store.counts).toEqual({ downloading: 0, waiting: 1, stopped: 0, finished: 0 });

		store.filter.text = '';
		store.filter.status = 'stopped';
		expect(store.groups.map((g) => g.label)).toEqual(['Stopped / failed']);
		// Counts keep every group so the status chips can show them.
		expect(store.counts.finished).toBe(1);

		// Dates are the last download, or when added for a task never downloaded.
		store.filter.status = 'all';
		store.filter.from = '2026-10-02';
		store.filter.to = '2026-10-06';
		expect(store.groups.flatMap((g) => titles(g.tasks))).toEqual(['Blame!']);
		store.filter.from = '';
		store.filter.to = '2026-10-01';
		expect(store.groups.flatMap((g) => titles(g.tasks))).toHaveLength(5);
	});

	it('applies progress frames and sums the speed of the downloading tasks', () => {
		const store = new QueueStore();
		store.load(queue());
		expect(store.rate).toBe(2_000_000);

		store.progress(progress({ id: 1, done: 12, total: 20, bytes_per_sec: 3_000_000 }));
		const kagurabachi = store.tasks.find((t) => t.id === 1);
		expect(kagurabachi?.done).toBe(12);
		expect(kagurabachi?.chapters).toBe('Ch. 98 (2/2)');
		expect(store.rate).toBe(3_500_000);

		// A task leaving the downloading group stops counting towards the speed.
		store.status({ id: 2, status: 'finished', error: null });
		expect(store.rate).toBe(3_000_000);
		expect(store.groups.at(-1)?.tasks.map((t) => t.id)).toEqual([2, 6]);
	});

	it('keeps the speed history of the last samples, one per sample() call', () => {
		const store = new QueueStore({ historySize: 3 });
		store.load(queue());
		store.sample();
		store.progress(progress({ id: 1, bytes_per_sec: 1_000_000 }));
		store.sample();
		store.status({ id: 1, status: 'stopped', error: null });
		store.status({ id: 2, status: 'stopped', error: null });
		store.sample();
		store.sample();
		expect(store.history).toEqual([1_500_000, 0, 0]);
	});

	it('records a failed status with its error', () => {
		const store = new QueueStore();
		store.load(queue());
		store.status({ id: 3, status: 'failed', error: 'page 13 missing' });
		const blueLock = store.tasks.find((t) => t.id === 3);
		expect(blueLock?.status).toBe('failed');
		expect(blueLock?.error).toBe('page 13 missing');
	});

	it('removes deleted tasks', () => {
		const store = new QueueStore();
		store.load(queue());
		store.removed(3);
		expect(store.tasks.map((t) => t.id)).toEqual([1, 2, 4, 5, 6]);
	});

	it('refetches the queue, debounced, when a frame is about a task it does not know', async () => {
		const fresh = [...queue(), task({ id: 7, title: 'Dandadan', status: 'waiting' })];
		const refresh = vi.fn(() => Promise.resolve(fresh));
		const store = new QueueStore({ refresh });
		store.load(queue());

		store.status({ id: 7, status: 'waiting', error: null });
		store.progress(progress({ id: 7 }));
		store.reordered();
		expect(refresh).not.toHaveBeenCalled();
		await vi.runAllTimersAsync();
		expect(refresh).toHaveBeenCalledOnce();
		expect(store.groups[1]?.tasks.map((t) => t.title)).toEqual(['Blue Lock', 'Dandadan']);
	});

	it('keeps tasks a frame changed while a refetch was in flight', async () => {
		let answer: (tasks: TaskSummary[]) => void = () => {};
		const refresh = vi.fn(() => new Promise<TaskSummary[]>((resolve) => (answer = resolve)));
		const store = new QueueStore({ refresh });
		store.load(queue());
		store.reordered();
		await vi.runAllTimersAsync();
		// The refetched list is older than this frame.
		store.progress(progress({ id: 1, done: 19 }));
		answer(queue());
		await vi.runAllTimersAsync();
		expect(store.tasks.find((t) => t.id === 1)?.done).toBe(19);
	});

	it('keeps an action’s answer over a refetch that was in flight', async () => {
		let answer: (tasks: TaskSummary[]) => void = () => {};
		const refresh = vi.fn(() => new Promise<TaskSummary[]>((resolve) => (answer = resolve)));
		const store = new QueueStore({ refresh });
		store.load(queue());
		store.reordered();
		await vi.runAllTimersAsync();
		store.upsert(task({ id: 3, title: 'Blue Lock', status: 'stopped' }));
		answer(queue());
		await vi.runAllTimersAsync();
		expect(store.tasks.find((t) => t.id === 3)?.status).toBe('stopped');
	});
});
