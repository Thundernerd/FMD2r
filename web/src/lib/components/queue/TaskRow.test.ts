// @vitest-environment jsdom
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Api } from '#lib/api/client.ts';
import type { TaskSummary } from '#lib/api/types.ts';
import { QueueStore } from '#lib/queue.svelte.ts';
import TaskRow from './TaskRow.svelte';

/** An 18Kami task: the module ID is the one fixtures/lua/modules/18Kami.lua:7 registers. */
const MODULE_ID = '73bedd818a034fcfb50144ff26298368';

const task: TaskSummary = {
	id: 1,
	module_id: MODULE_ID,
	link: '/album/1',
	title: 'Kagurabachi',
	status: 'stopped',
	enabled: true,
	running: false,
	error: null,
	chapters: 'Chapter 1',
	chapters_done: 0,
	chapter_count: 1,
	current_chapter: 0,
	done: 0,
	total: 0,
	bytes_per_sec: 0,
	save_to: '/data/downloads/Kagurabachi',
	date_added: '2026-01-01T00:00:00Z',
	date_last_downloaded: null
};

const api = { taskFilesUrl: (id: number) => `/api/tasks/${id}/files` } as Pick<
	Api,
	'taskFilesUrl'
> as Api;

function renderRow(websites: ReadonlyMap<string, string>) {
	render(TaskRow, { task, api, queue: new QueueStore(), websites, onerror: () => {} });
	return screen.getByRole('listitem', { name: 'Kagurabachi' });
}

describe('TaskRow', () => {
	it("shows the task's website name instead of its module ID", () => {
		const row = renderRow(new Map([[MODULE_ID, '18Kami']]));
		expect(row.textContent).toContain('18Kami');
		expect(row.textContent).not.toContain(MODULE_ID);
	});

	it('shows neither a website nor the module ID when the module is unknown', () => {
		const row = renderRow(new Map([['other', '18Kami']]));
		expect(row.textContent).not.toContain('18Kami');
		expect(row.textContent).not.toContain(MODULE_ID);
	});
});

describe('TaskRow reorder buttons', () => {
	const waiting = (id: number, title: string): TaskSummary => ({
		...task,
		id,
		title,
		status: 'waiting'
	});
	/** Three waiting tasks around a downloading and a finished one. */
	const tasks: TaskSummary[] = [
		{ ...task, id: 1, title: 'Berserk', status: 'downloading' },
		waiting(2, 'Blue Lock'),
		waiting(3, 'Vagabond'),
		{ ...task, id: 4, title: 'Blame!', status: 'finished' },
		waiting(5, 'Dorohedoro')
	];

	function renderWaiting(title: string, reorderTasks: Api['reorderTasks'], onerror = () => {}) {
		const queue = new QueueStore();
		queue.load(tasks);
		const row = tasks.find((t) => t.title === title);
		if (!row) throw new Error(title);
		render(TaskRow, {
			task: row,
			api: { ...api, reorderTasks } as Api,
			queue,
			websites: new Map(),
			reorderable: true,
			onerror
		});
		const item = within(screen.getByRole('listitem', { name: title }));
		return (label: string) => item.getByRole('button', { name: label }) as HTMLButtonElement;
	}

	it("disables the first waiting row's Move up and Move to top", () => {
		const button = renderWaiting('Blue Lock', vi.fn());
		expect(button('Move to top').disabled).toBe(true);
		expect(button('Move up').disabled).toBe(true);
		expect(button('Move down').disabled).toBe(false);
		expect(button('Move to bottom').disabled).toBe(false);
	});

	it("disables the last waiting row's Move down and Move to bottom", () => {
		const button = renderWaiting('Dorohedoro', vi.fn());
		expect(button('Move to top').disabled).toBe(false);
		expect(button('Move up').disabled).toBe(false);
		expect(button('Move down').disabled).toBe(true);
		expect(button('Move to bottom').disabled).toBe(true);
	});

	it('sends the whole queue with the task moved among the waiting tasks', async () => {
		const reorderTasks = vi.fn(async () => {});
		const button = renderWaiting('Dorohedoro', reorderTasks);
		await fireEvent.click(button('Move to top'));
		expect(reorderTasks).toHaveBeenCalledWith([1, 5, 2, 4, 3]);
	});

	it('reports a failed reorder', async () => {
		const onerror = vi.fn();
		const button = renderWaiting(
			'Vagabond',
			vi.fn(async () => {
				throw new Error('offline');
			}),
			onerror
		);
		await fireEvent.click(button('Move up'));
		await vi.waitFor(() => expect(onerror).toHaveBeenCalledWith('Could not reorder the queue.'));
	});

	it('has no reorder buttons on rows outside the Waiting group', () => {
		const queue = new QueueStore();
		queue.load(tasks);
		const [downloading] = tasks;
		if (!downloading) throw new Error('no tasks');
		render(TaskRow, {
			task: downloading,
			api,
			queue,
			websites: new Map(),
			reorderable: true,
			onerror: () => {}
		});
		const item = within(screen.getByRole('listitem', { name: 'Berserk' }));
		expect(item.queryByRole('button', { name: 'Move up' })).toBeNull();
	});
});
