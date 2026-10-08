import type { EventSourceLike } from '#lib/events.svelte.ts';
import { createMockLists } from './mock-lists';
import { Invalid, createMockSettings } from './mock-settings';
import type { paths } from './schema';
import type {
	About,
	InboxItem,
	JobState,
	LogLevel,
	LogLine,
	ModuleSummary,
	SaveToSettings,
	SeriesRef,
	TaskProgress
} from './types';

type ResolveBody = paths['/api/resolve']['post']['requestBody']['content']['application/json'];

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

const HOUR_MS = 3_600_000;

const seedJobs = (now: number): JobState[] => [
	{
		id: 'favorites',
		title: 'Check favorites',
		state: 'running',
		done: 31,
		total: 48,
		last_run: new Date(now - 2 * 60_000).toISOString(),
		next_run: new Date(now + HOUR_MS).toISOString(),
		last_error: null
	},
	{
		id: 'lists',
		title: 'Update lists',
		state: 'idle',
		done: 0,
		total: 0,
		last_run: new Date(now - 20 * HOUR_MS).toISOString(),
		next_run: new Date(now + 4 * HOUR_MS).toISOString(),
		last_error: null
	},
	{
		id: 'modules',
		title: 'Update modules',
		state: 'failed',
		done: 212,
		total: 665,
		last_run: new Date(now - 3 * HOUR_MS).toISOString(),
		next_run: new Date(now + 21 * HOUR_MS).toISOString(),
		last_error:
			'GitHub API: HTTP 403 rate limit exceeded\nRetry after 2026-10-08T10:00:00Z (X-RateLimit-Reset).'
	}
];

/** How many items a mock job run works through. */
const JOB_RUN_SIZE: Record<string, number> = { favorites: 48, lists: 40, modules: 665 };

const seedAbout = (): About => ({
	version: '0.1.0',
	git_revision: '2780af1e7ede',
	upstream_ref: 'master',
	upstream_sha: '4f2c9e1b7a30',
	module_count: 665,
	load_failures: [
		{
			module: 'Batoto.lua',
			error: "attempt to call a nil value (field 'ResolveRedirect')",
			inbox_id: 'batoto-host-api'
		}
	],
	xpath_backend: 'fpc',
	data_dir: '/data',
	databases: [
		{ name: 'app.db', bytes: 2_412_544 },
		{ name: 'lists.db', bytes: 318_767_104 }
	],
	uptime_secs: 93_784,
	tools: [
		{ name: 'python3', ok: true, detail: 'Python 3.12.3' },
		{ name: 'node', ok: true, detail: 'v22.4.0' },
		{ name: 'magick', ok: false, detail: 'not found on PATH' },
		{ name: 'FlareSolverr', ok: false, detail: 'localhost:8191: Connection refused (os error 111)' }
	]
});

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
	/** A fake `/api/events` stream that advances tasks and running jobs, logs, and posts one inbox item after 30 s. */
	eventSource: (url: string) => EventSourceLike;
}

export function createMockBackend(): MockBackend {
	const inbox = seedInbox();
	const tasks = seedTasks();
	const jobs = seedJobs(Date.now());
	const about = seedAbout();
	const logs: LogLine[] = [];
	let logSeq = 0;
	const log = (level: LogLevel, target: string, module: string | null, message: string) => {
		const line: LogLine = {
			seq: ++logSeq,
			time: new Date().toISOString(),
			level,
			target,
			module,
			message
		};
		logs.push(line);
		if (logs.length > 2000) logs.shift();
		return line;
	};
	log('INFO', 'fmd_server', null, 'listening on 0.0.0.0:8080');
	for (let n = 1; n <= 40; n++) {
		log('DEBUG', 'fmd_lua', 'MangaDex', `GET https://api.mangadex.org/at-home/server/${n}`);
		if (n % 8 === 0) log('WARN', 'fmd.logger', 'MangaDex', 'rate limited, retrying in 2s');
	}
	log('ERROR', 'fmd.logger', 'Bato.to', "attempt to call a nil value (field 'ResolveRedirect')");
	log('INFO', 'fmd_core::jobs', null, 'favorites check started');

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

	const settings = createMockSettings();
	const lists = createMockLists();
	const modules = (): ModuleSummary[] =>
		settings.listModules().map((m) => ({
			...m,
			capabilities: { update_list: true, info: true, download: true, account: false },
			...lists.summary(m.id)
		}));
	/** Runs a settings update, answering a rejected one the way fmd-server does. */
	const update = async (req: Request, apply: (patch: Record<string, unknown>) => unknown) => {
		const patch = (await req.json()) as unknown;
		if (typeof patch !== 'object' || patch === null || Array.isArray(patch)) {
			return json({ status: 400, detail: 'expected a JSON object' }, 400);
		}
		try {
			const result = apply(patch as Record<string, unknown>);
			return result === null ? new Response(null, { status: 404 }) : json(result);
		} catch (e) {
			if (!(e instanceof Invalid)) throw e;
			return json(
				{ status: 422, title: 'Unprocessable Entity', detail: e.detail, field: e.field },
				422
			);
		}
	};

	const fetch = async (req: Request): Promise<Response> => {
		const { pathname, searchParams } = new URL(req.url);
		const route = `${req.method} ${pathname}`;

		if (route === 'GET /api/inbox') return json(inbox);
		if (route === 'GET /api/tasks') return json(tasks);
		if (route === 'GET /api/logs') return json(logs);
		if (route === 'GET /api/jobs') return json(jobs);
		if (route === 'GET /api/settings') return json(settings.getSettings());
		if (route === 'PATCH /api/settings') return update(req, settings.patchSettings);
		if (route === 'POST /api/preview-rename') {
			return json(settings.previewRename((await req.json()) as SaveToSettings));
		}
		if (route === 'GET /api/modules') return json(modules());
		if (route === 'GET /api/lists/search') return json(lists.search(searchParams));
		if (route === 'GET /api/lists/facets') return json(lists.facets(searchParams));
		const listJob = /^POST \/api\/lists\/([^/]+)\/(update|import-db|cancel)$/.exec(route);
		if (listJob?.[1]) {
			const module = decodeURIComponent(listJob[1]);
			if (!modules().some((m) => m.id === module)) return json({ status: 404 }, 404);
			if (listJob[2] === 'cancel') {
				return lists.cancel(module)
					? new Response(null, { status: 202 })
					: json({ status: 409, detail: 'no list job is running' }, 409);
			}
			const job = listJob[2] === 'update' ? 'update' : 'import_db';
			if (!lists.start(module, job)) {
				return json({ status: 409, detail: 'a list job is already running' }, 409);
			}
			return json({ module_id: module, job }, 202);
		}
		const moduleSettings = /^(GET|PATCH) \/api\/modules\/([^/]+)\/settings$/.exec(route);
		if (moduleSettings?.[2]) {
			const id = decodeURIComponent(moduleSettings[2]);
			if (moduleSettings[1] === 'PATCH') {
				return update(req, (patch) => settings.patchModule(id, patch));
			}
			const view = settings.getModule(id);
			return view ? json(view) : new Response(null, { status: 404 });
		}
		if (route === 'GET /api/about') {
			return json({
				...about,
				uptime_secs: about.uptime_secs + Math.round(performance.now() / 1000)
			});
		}

		const control = /^POST \/api\/jobs\/([^/]+)\/(run|cancel)$/.exec(route);
		if (control?.[1]) {
			const job = jobs.find((j) => j.id === decodeURIComponent(control[1] ?? ''));
			if (!job) return json({ status: 404, title: 'Not Found' }, 404);
			const running = job.state === 'running';
			if (control[2] === 'run') {
				if (running) return json({ status: 409, detail: 'job is already running' }, 409);
				Object.assign(job, {
					state: 'running',
					done: 0,
					total: JOB_RUN_SIZE[job.id] ?? 10,
					last_run: new Date().toISOString(),
					last_error: null
				});
				log('INFO', 'fmd_core::jobs', null, `${job.title} started`);
			} else {
				if (!running) return json({ status: 409, detail: 'job is not running' }, 409);
				job.state = 'idle';
				log('INFO', 'fmd_core::jobs', null, `${job.title} cancelled`);
			}
			return json(job, 202);
		}

		const read = /^POST \/api\/inbox\/([^/]+)\/read$/.exec(route);
		if (read?.[1]) {
			const item = inbox.find((i) => i.id === decodeURIComponent(read[1] ?? ''));
			if (!item) return new Response(null, { status: 404 });
			item.read = true;
			return new Response(null, { status: 204 });
		}

		if (route === 'POST /api/resolve') {
			// Untrusted input: check the shape instead of trusting the generated type.
			const body = (await req.json()) as Partial<ResolveBody> | null;
			const ref = typeof body?.url === 'string' ? resolve(body.url) : null;
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

		let ticks = 0;
		const tick = () => {
			ticks++;
			for (const task of tasks) {
				if (task.status !== 'downloading') continue;
				task.done = task.done >= task.total ? 0 : task.done + 1;
				task.bytes_per_sec = Math.round(1_500_000 + Math.random() * 1_500_000);
				emit('task.progress', task);
				emit(
					'log',
					log(
						'INFO',
						'fmd.logger',
						'MangaDex',
						`${task.title}: page ${task.done}/${task.total} saved`
					)
				);
			}
			for (const job of jobs) {
				if (job.state !== 'running') continue;
				job.done = Math.min(job.done + Math.ceil(job.total / 20), job.total);
				if (job.done === job.total) {
					job.state = 'done';
					emit('log', log('INFO', 'fmd_core::jobs', null, `${job.title} finished`));
				}
				emit('job.state', job);
			}
			lists.tick((event) => emit(`job.lists.${event.kind}`, event));
			// Late enough not to disturb the smoke tests, early enough to see in `npm run dev:mock`.
			if (ticks === 30) {
				const item: InboxItem = {
					id: 'download-failed',
					kind: 'error',
					title: 'Blue Lock Ch. 3 failed',
					body: 'HTTP 403 from the image host after 3 retries.',
					created_at: new Date().toISOString(),
					read: false
				};
				inbox.unshift(item);
				emit('inbox.new', item);
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
