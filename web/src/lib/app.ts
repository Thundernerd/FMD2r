import { createApi } from '#lib/api/client.ts';
import { createMockBackend } from '#lib/api/mock.ts';
import { createEventStore } from '#lib/events.svelte.ts';

/** `VITE_API_MOCK=true` runs the UI against an in-memory backend (see README.md). */
const mock = import.meta.env.VITE_API_MOCK === 'true' ? createMockBackend() : null;

export const api = createApi(mock ? { fetch: mock.fetch } : {});

export const events = createEventStore({
	url: '/api/events',
	connect: mock ? mock.eventSource : (url) => new EventSource(url)
});
