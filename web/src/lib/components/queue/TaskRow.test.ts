// @vitest-environment jsdom
import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
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
