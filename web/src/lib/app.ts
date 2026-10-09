import { createApi } from '#lib/api/client.ts';
import { createMockBackend } from '#lib/api/mock.ts';
import { EventStore } from '#lib/events.svelte.ts';
import { QueueStore } from '#lib/queue.svelte.ts';

/** `VITE_API_MOCK=true` runs the UI against an in-memory backend (see README.md). */
const mock = import.meta.env.VITE_API_MOCK === 'true' ? createMockBackend() : null;

export const api = createApi(
	mock
		? { fetch: mock.fetch, taskFilesUrl: mock.taskFilesUrl, logsDownloadUrl: mock.logsDownloadUrl }
		: {}
);

export const events = new EventStore({
	url: '/api/events',
	connect: mock ? mock.eventSource : (url) => new EventSource(url),
	queue: new QueueStore({ refresh: () => api.listTasks() })
});
