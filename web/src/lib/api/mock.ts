import type { EventSourceLike } from '#lib/events.svelte.ts';
import type { InboxItem, SeriesRef, TaskProgress } from './types';

// In-memory stand-in for fmd-server, used when VITE_API_MOCK=true (see README.md).
// Data mirrors the approved layout prototype so the chrome has something realistic to show.

const seedInbox = (): InboxItem[] => [
	{
		id: 'new-chapters',
		kind: 'warn',
		title: 'New chapters for 3 favorites',
		body: 'Kagurabachi (+2), Omniscient Reader’s Viewpoint (+3), Sakamoto Days (+8). Auto-download is off.',
		created_at: '2026-10-08T09:12:00Z',
		read: false
	},
	{
		id: 'module-updates',
		kind: 'info',
		title: '14 module updates ready',
		body: 'Updates apply live. Modules in use by a running download switch over when that download finishes.',
		created_at: '2026-10-08T08:00:00Z',
		read: false
	},
	{
		id: 'batoto-host-api',
		kind: 'error',
		title: 'Bato.to module needs a newer FMD2r',
		body: 'Module 3.1.2 calls fmd.net.ResolveRedirect, which this build does not provide. Bato.to is paused until FMD2r is updated.',
		created_at: '2026-10-07T21:00:00Z',
		read: false
	},
	{
		id: 'list-missing',
		kind: 'info',
		title: 'Bato.to has no manga list yet',
		body: 'Download the prebuilt list from FMD2-DB or build it from the website (slow).',
		created_at: '2026-10-07T20:00:00Z',
		read: true
	}
];

const seedTasks = (): TaskProgress[] => [
	{
		id: 1,
		title: 'Kagurabachi',
		chapters: 'Ch. 97–98',
		status: 'downloading',
		done: 22,
		total: 38,
		bytes_per_sec: 1_800_000
	},
	{
		id: 2,
		title: 'Omniscient Reader’s Viewpoint',
		chapters: 'Ch. 239–241',
		status: 'downloading',
		done: 51,
		total: 120,
		bytes_per_sec: 2_600_000
	},
	{
		id: 3,
		title: 'Blue Lock',
		chapters: 'Ch. 1–40',
		status: 'queued',
		done: 0,
		total: 840,
		bytes_per_sec: 0
	},
	{
		id: 4,
		title: 'The Apothecary Diaries',
		chapters: 'Ch. 61–76',
		status: 'failed',
		done: 140,
		total: 310,
		bytes_per_sec: 0
	}
];

/** Hosts the mock pretends to have modules for; anything else resolves to a 404. */
const MODULES: Record<string, string> = {
	'mangadex.org': 'MangaDex',
	'comick.io': 'ComicK',
	'bato.to': 'Bato.to',
	'www.webtoons.com': 'Webtoons'
};

const json = (body: unknown, status = 200): Response =>
	new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

export interface MockBackend {
	/** Answers `/api/*` requests from in-memory state. */
	fetch: (input: Request) => Promise<Response>;
	/** A fake `/api/events` stream that advances the downloading tasks once a second. */
	eventSource: (url: string) => EventSourceLike;
}

export function createMockBackend(): MockBackend {
	const inbox = seedInbox();
	const tasks = seedTasks();

	const resolve = (raw: string): SeriesRef | null => {
		let url: URL;
		try {
			url = new URL(raw);
		} catch {
			return null;
		}
		const module = MODULES[url.hostname];
		const link = (url.pathname + url.search).replace(/^\/+/, '');
		return module && link ? { module, link } : null;
	};

	const fetch = async (req: Request): Promise<Response> => {
		const { pathname } = new URL(req.url);
		const route = `${req.method} ${pathname}`;

		if (route === 'GET /api/inbox') return json(inbox);
		if (route === 'GET /api/tasks') return json(tasks);

		const read = /^POST \/api\/inbox\/([^/]+)\/read$/.exec(route);
		if (read?.[1]) {
			const item = inbox.find((i) => i.id === decodeURIComponent(read[1] ?? ''));
			if (!item) return new Response(null, { status: 404 });
			item.read = true;
			return new Response(null, { status: 204 });
		}

		if (route === 'POST /api/resolve') {
			const body = (await req.json()) as { url?: unknown };
			const ref = typeof body.url === 'string' ? resolve(body.url) : null;
			return ref ? json(ref) : new Response(null, { status: 404 });
		}

		return new Response(null, { status: 404 });
	};

	const eventSource = (): EventSourceLike => {
		const listeners = new Map<string, ((ev: MessageEvent<string>) => void)[]>();
		const emit = (type: string, payload: unknown) => {
			const ev = new MessageEvent<string>(type, { data: JSON.stringify(payload) });
			for (const listener of listeners.get(type) ?? []) listener(ev);
		};

		const tick = () => {
			for (const task of tasks) {
				if (task.status !== 'downloading') continue;
				task.done = task.done >= task.total ? 0 : task.done + 1;
				emit('task.progress', task);
			}
		};

		const es: EventSourceLike = {
			onopen: null,
			onerror: null,
			addEventListener(type, listener) {
				listeners.set(type, [...(listeners.get(type) ?? []), listener]);
			},
			close() {
				clearTimeout(opening);
				clearInterval(timer);
			}
		};
		const opening = setTimeout(() => {
			es.onopen?.(new Event('open'));
			for (const task of tasks) emit('task.progress', task);
		}, 0);
		const timer = setInterval(tick, 1000);
		return es;
	};

	return { fetch, eventSource };
}
