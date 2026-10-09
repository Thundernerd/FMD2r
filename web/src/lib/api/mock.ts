import type { EventSourceLike } from '#lib/events.svelte.ts';
import { createMockFavorites } from './mock-favorites';
import { mockImportReport } from './mock-import';
import { createMockLists } from './mock-lists';
import { Invalid, createMockSettings } from './mock-settings';
import type { paths } from './schema';
import type {
	About,
	AccountInfo,
	AccountRequest,
	FavoritePatch,
	InboxItem,
	JobState,
	LogLevel,
	LogLine,
	ModuleSummary,
	NewTask,
	RenamePreviewRequest,
	SeriesInfo,
	SeriesRef,
	TaskDetail,
	TaskOrder,
	TaskState,
	TaskSummary
} from './types';
import type { TaskAction } from './client';
import { groupOf } from '#lib/queue.svelte.ts';

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

const DAY_MS = 86_400_000;

const TASK_ACTIONS: readonly string[] = [
	'start',
	'stop',
	'redownload',
	'enable',
	'disable'
] satisfies TaskAction[];
const isTaskAction = (action: string): action is TaskAction => TASK_ACTIONS.includes(action);

/** A queued task as fmd-server lists it; `chapter_count` chapters, `chapters_done` of them done. */
const mockTask = (
	over: Partial<TaskSummary> & Pick<TaskSummary, 'id' | 'title' | 'status' | 'chapter_count'>,
	now: number
): TaskSummary => ({
	module_id: 'mangadex',
	link: `/title/mock-${over.id}`,
	save_to: `/data/downloads/${over.title}`,
	enabled: over.status !== 'disabled',
	running: over.status === 'downloading',
	error: null,
	chapters: '',
	chapters_done: 0,
	current_chapter: over.chapters_done ?? 0,
	done: 0,
	total: 20,
	bytes_per_sec: 0,
	date_added: new Date(now - over.id * DAY_MS).toISOString(),
	date_last_downloaded: null,
	...over
});

const seedTasks = (now: number): TaskSummary[] =>
	[
		mockTask(
			{
				id: 1,
				title: 'Kagurabachi',
				status: 'downloading',
				chapter_count: 2,
				done: 12,
				total: 19,
				bytes_per_sec: 1_800_000
			},
			now
		),
		mockTask(
			{
				id: 2,
				title: 'Omniscient Reader’s Viewpoint',
				status: 'downloading',
				chapter_count: 3,
				chapters_done: 1,
				done: 5,
				total: 40,
				bytes_per_sec: 2_600_000
			},
			now
		),
		mockTask({ id: 3, title: 'Blue Lock', status: 'waiting', chapter_count: 40 }, now),
		mockTask(
			{
				id: 4,
				title: 'The Apothecary Diaries',
				status: 'failed',
				chapter_count: 16,
				chapters_done: 7,
				done: 14,
				total: 31,
				error: 'HTTP 403 from the image host after 3 retries'
			},
			now
		),
		mockTask({ id: 5, title: 'Sakamoto Days', status: 'stopped', chapter_count: 8 }, now),
		mockTask(
			{
				id: 6,
				title: 'Frieren',
				status: 'finished',
				chapter_count: 4,
				chapters_done: 4,
				done: 18,
				total: 18,
				date_last_downloaded: new Date(now - 2 * DAY_MS).toISOString()
			},
			now
		),
		mockTask(
			{
				id: 7,
				title: 'Dandadan',
				module_id: 'comick',
				status: 'finished',
				chapter_count: 1,
				chapters_done: 1,
				done: 22,
				total: 22,
				date_last_downloaded: new Date(now - 30 * DAY_MS).toISOString()
			},
			now
		)
	].map((t) => ({ ...t, chapters: chapterLabel(t) }));

/** The chapter a task is at, as fmd-server labels it. */
const chapterLabel = (task: TaskSummary): string => {
	const index = Math.min(task.current_chapter, task.chapter_count - 1);
	const name = `Chapter ${index + 1}`;
	return task.chapter_count > 1 ? `${name} (${index + 1}/${task.chapter_count})` : name;
};

const HOUR_MS = 3_600_000;

const seedJobs = (now: number): JobState[] => [
	{
		id: 'favorites',
		title: 'Check favorites',
		state: 'done',
		done: 8,
		total: 8,
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
const JOB_RUN_SIZE: Record<string, number> = { lists: 40, modules: 665 };

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
	xpath_backend: 'native',
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

/** Hosts the mock pretends to have modules for (by module ID); anything else resolves to a 404. */
const MODULES: Record<string, string> = {
	'mangadex.org': 'mangadex',
	'comick.io': 'comick',
	'bato.to': 'batoto',
	'www.webtoons.com': 'webtoons'
};

/** Series the mock knows, by link; `chapters` of them, the first `downloaded` seen. */
const SERIES: Record<
	string,
	Omit<SeriesInfo, 'module_id' | 'link' | 'chapters'> & {
		count: number;
		downloaded: number;
	}
> = {
	'/title/abc123/frieren': {
		title: 'Frieren',
		alt_titles: "Sousou no Frieren, Frieren: Beyond Journey's End",
		authors: 'Kanehito Yamada',
		artists: 'Tsukasa Abe',
		genres: ['Adventure', 'Drama', 'Fantasy', 'Shounen'],
		status: 'ongoing',
		summary:
			'The demon king is dead and the hero party has gone home. Frieren, the elf mage who outlives them all, sets out to understand the people she travelled with.\r\nDecades later, she retraces their journey with a new apprentice, visiting the places they saved and the graves of the friends she barely got to know.\r\nAlong the way she learns what a short human life is worth, and why her companions bothered to spend theirs with her.',
		cover_url: null,
		in_library: true,
		count: 142,
		downloaded: 138
	},
	'/title/op/one-piece': {
		title: 'One Piece',
		alt_titles: 'ワンピース',
		authors: 'Eiichiro Oda',
		artists: 'Eiichiro Oda',
		genres: ['Action', 'Adventure', 'Comedy', 'Shounen'],
		status: 'ongoing',
		summary: 'Gol D. Roger was the King of the Pirates. His treasure is still out there.',
		cover_url: null,
		in_library: false,
		count: 2000,
		downloaded: 0
	}
};

const json = (body: unknown, status = 200): Response =>
	new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

export interface MockBackend {
	/** Answers `/api/*` requests from in-memory state. */
	fetch: (input: Request) => Promise<Response>;
	/** A fake `/api/events` stream that advances tasks and running jobs, logs, and posts one inbox item after 30 s. */
	eventSource: (url: string) => EventSourceLike;
	/** A small fake archive for a task's "Get files" link (there is no server to link to). */
	taskFilesUrl: (id: number) => string;
	logsDownloadUrl: () => string;
}

export interface MockOptions {
	/**
	 * The password the mock requires, like `fmd2r serve --password`; `null` turns auth off.
	 * Defaults to `sessionStorage['fmd2r.mock.password']`, so a test can turn auth on.
	 */
	password?: string | null;
}

const PASSWORD_KEY = 'fmd2r.mock.password';
const SESSION_KEY = 'fmd2r.mock.session';

/** A sessionStorage item, or `null` without storage (tests, private mode). */
const stored = (key: string): string | null => {
	try {
		return globalThis.sessionStorage?.getItem(key) ?? null;
	} catch {
		return null;
	}
};

/** Sets (or, with `null`, removes) a sessionStorage item; does nothing without storage. */
const store = (key: string, value: string | null) => {
	try {
		if (value === null) globalThis.sessionStorage?.removeItem(key);
		else globalThis.sessionStorage?.setItem(key, value);
	} catch {
		// Not persisted; the in-memory state still works.
	}
};

export function createMockBackend({
	password = stored(PASSWORD_KEY)
}: MockOptions = {}): MockBackend {
	/** Whether this tab holds a session; kept in sessionStorage so it survives a reload, like the cookie. */
	let loggedIn = stored(SESSION_KEY) !== null;
	const setLoggedIn = (value: boolean) => {
		loggedIn = value;
		store(SESSION_KEY, value ? '1' : null);
	};
	const inbox = seedInbox();
	let tasks = seedTasks(Date.now());
	/** Every open fake event stream, so API calls can announce what they changed. */
	const streams = new Set<(type: string, payload: unknown) => void>();
	const broadcast = (type: string, payload: unknown) => {
		for (const emit of streams) emit(type, payload);
	};
	const jobs = seedJobs(Date.now());
	const about = seedAbout();
	const favoritesJob = jobs.find((j) => j.id === 'favorites');
	if (!favoritesJob) throw new Error('the mock jobs lack the favorites check');
	const favorites = createMockFavorites(favoritesJob);
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
	log('INFO', 'fmd_server', null, 'listening on 127.0.0.1:8080');
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
		const module_id = MODULES[url.hostname];
		// Like fmd-server: the path (and query) relative to the module's RootURL.
		const link = url.pathname + url.search;
		return module_id && link !== '/' ? { module_id, link } : null;
	};

	const series = (module_id: string, link: string): SeriesInfo | null => {
		const known = SERIES[link];
		if (!known || !Object.values(MODULES).includes(module_id)) return null;
		const { count, downloaded, ...info } = known;
		// Oldest first, as FMD2 modules list chapters.
		const chapters = Array.from({ length: count }, (_, i) => ({
			name: `Chapter ${i + 1}`,
			link: `${link}/chapter/${i + 1}`,
			downloaded: i < downloaded
		}));
		return { ...info, module_id, link, chapters, in_library: favorites.has(module_id, link) };
	};

	let nextTaskId = 100;
	/** Queues `task` as fmd-server does: one waiting task holding the chosen chapters. */
	const createTask = (task: NewTask): TaskSummary => {
		const queued = mockTask(
			{
				id: nextTaskId++,
				title: task.title,
				module_id: task.module_id,
				link: task.link,
				status: 'waiting',
				chapter_count: task.chapters.length,
				total: 0,
				date_added: new Date().toISOString()
			},
			Date.now()
		);
		queued.chapters = chapterLabel(queued);
		tasks.push(queued);
		broadcast('task.status', { id: queued.id, status: queued.status, error: null });
		return queued;
	};

	const setStatus = (task: TaskSummary, status: TaskState, error: string | null = null) => {
		if (task.status === status) return;
		task.status = status;
		task.error = error;
		task.running = groupOf(status) === 'downloading';
		if (!task.running) task.bytes_per_sec = 0;
		broadcast('task.status', { id: task.id, status, error });
	};

	/** `POST /api/tasks/{id}/<action>`, following FMD2's `TDownloadManager` rules. */
	const act = (task: TaskSummary, action: TaskAction): void => {
		switch (action) {
			case 'start':
				if (task.enabled && ['stopped', 'failed'].includes(task.status)) setStatus(task, 'waiting');
				return;
			case 'stop':
				if (['waiting', 'downloading'].includes(groupOf(task.status))) {
					setStatus(task, 'stopped');
				}
				return;
			case 'redownload':
				if (task.enabled && task.status !== 'waiting' && !task.running) {
					Object.assign(task, { chapters_done: 0, current_chapter: 0, done: 0 });
					task.chapters = chapterLabel(task);
					setStatus(task, 'waiting');
				}
				return;
			case 'enable':
				if (!task.enabled) {
					task.enabled = true;
					setStatus(task, 'stopped');
				}
				return;
			case 'disable':
				if (task.enabled) {
					task.enabled = false;
					setStatus(task, 'disabled');
				}
				return;
		}
	};

	const detail = (task: TaskSummary): TaskDetail => ({
		task,
		chapters: Array.from({ length: task.chapter_count }, (_, i) => ({
			index: i,
			name: `Chapter ${i + 1}`,
			link: `${task.link}/chapter/${i + 1}`,
			status: i < task.chapters_done ? 'downloaded' : 'pending',
			done: i < task.chapters_done ? 20 : i === task.current_chapter ? task.done : 0,
			total: i <= task.current_chapter ? 20 : 0
		}))
	});

	const remove = (task: TaskSummary) => {
		tasks = tasks.filter((t) => t.id !== task.id);
		broadcast('task.removed', { id: task.id });
	};

	const taskFilesUrl = (id: number) => {
		const task = tasks.find((t) => t.id === id);
		const zip = (task?.chapter_count ?? 1) > 1;
		const blob = new Blob([`mock files of task ${id}`], {
			type: zip ? 'application/zip' : 'application/vnd.comicbook+zip'
		});
		return URL.createObjectURL(blob);
	};

	const settings = createMockSettings();
	const lists = createMockLists(() => settings.getSettings().general.selected_websites);
	const modules = (): ModuleSummary[] =>
		settings.listModules().map((m) => ({
			...m,
			capabilities: { update_list: true, info: true, download: true, account: false },
			...lists.summary(m.id)
		}));
	// Login succeeds for any non-empty username and password, like a module that accepts them.
	const accounts: Record<string, AccountInfo> = {
		ehentai: {
			module: 'ehentai',
			name: 'E-Hentai',
			enabled: true,
			username: 'reader',
			has_password: true,
			status: 'valid'
		},
		madokami: {
			module: 'madokami',
			name: 'Madokami',
			enabled: false,
			username: '',
			has_password: false,
			status: 'unknown'
		}
	};
	/** The write-only passwords, kept apart so no answer can carry one. */
	const passwords: Record<string, string> = { ehentai: 'secret', madokami: '' };
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
				{
					status: 422,
					title: 'Unprocessable Entity',
					detail: e.message,
					field: e.fields[0]?.field,
					fields: e.fields
				},
				422
			);
		}
	};

	/** The `import` job, listed once an import ran, as fmd-server registers it. */
	const importJob: JobState = {
		id: 'import',
		title: 'Import from FMD2',
		state: 'idle',
		done: 0,
		total: 5,
		last_run: null,
		next_run: null,
		last_error: null
	};
	/**
	 * `POST /api/import`: anything starting like a zip is the mock's FMD2 userdata; an import adds
	 * its favorite, One Piece, to the library.
	 */
	const importUserdata = async (req: Request, query: URLSearchParams): Promise<Response> => {
		if (importJob.state === 'running') {
			return json({ status: 409, detail: 'job is already running' }, 409);
		}
		const zip = new Uint8Array(await req.arrayBuffer());
		if (zip[0] !== 0x50 || zip[1] !== 0x4b) {
			return json({ status: 400, detail: 'not a zip file: invalid Zip archive' }, 400);
		}
		const zone = query.get('timezone');
		if (zone && !Intl.supportedValuesOf('timeZone').includes(zone)) {
			return json({ status: 422, detail: `unknown time zone "${zone}"`, field: 'timezone' }, 422);
		}
		if (!jobs.includes(importJob)) jobs.push(importJob);
		Object.assign(importJob, { state: 'running', done: 0, last_run: new Date().toISOString() });
		broadcast('job.state', importJob);
		for (let step = 1; step <= importJob.total; step++) {
			await new Promise((resolve) => setTimeout(resolve, 150));
			importJob.done = step;
			broadcast('job.state', importJob);
		}
		const dryRun = query.get('dry_run') === 'true';
		const onePiece = series('mangadex', '/title/op/one-piece');
		const exists = favorites.has('mangadex', '/title/op/one-piece');
		if (!dryRun && onePiece && !exists) favorites.add(onePiece, 'MangaDex');
		importJob.state = 'done';
		broadcast('job.state', importJob);
		return json(mockImportReport(dryRun, query.getAll('map_path'), exists));
	};

	const fetch = async (req: Request): Promise<Response> => {
		const { pathname, searchParams } = new URL(req.url);
		const route = `${req.method} ${pathname}`;

		if (route === 'GET /api/health')
			return json({ status: 'ok', auth: password !== null, loopback: true, overridden: [] });
		if (route === 'POST /api/login') {
			const body = (await req.json()) as { password?: unknown } | null;
			if (password !== null && body?.password !== password) {
				return json({ status: 401, title: 'Unauthorized' }, 401);
			}
			if (password !== null) setLoggedIn(true);
			return new Response(null, { status: 204 });
		}
		if (route === 'POST /api/logout') {
			setLoggedIn(false);
			return new Response(null, { status: 204 });
		}
		if (password !== null && !loggedIn) {
			return json({ status: 401, title: 'Unauthorized' }, 401);
		}
		if (route === 'GET /api/inbox') return json(inbox);
		if (route === 'GET /api/tasks') {
			const counts = { downloading: 0, waiting: 0, stopped: 0, finished: 0 };
			for (const task of tasks) counts[groupOf(task.status)]++;
			const page = Number(searchParams.get('page') ?? 1);
			const perPage = Number(searchParams.get('per_page') ?? 100);
			const items = tasks.slice((page - 1) * perPage, page * perPage);
			return json({ items, total: tasks.length, page, per_page: perPage, counts });
		}
		if (route === 'POST /api/tasks/start-all' || route === 'POST /api/tasks/stop-all') {
			for (const task of tasks) {
				if (route.endsWith('start-all')) {
					if (task.status !== 'finished' && task.enabled && !task.running) {
						setStatus(task, 'waiting');
					}
				} else act(task, 'stop');
			}
			return new Response(null, { status: 204 });
		}
		if (route === 'POST /api/tasks/reorder') {
			const { ids } = (await req.json()) as TaskOrder;
			const first = ids.flatMap((id) => tasks.filter((t) => t.id === id));
			tasks = [...first, ...tasks.filter((t) => !ids.includes(t.id))];
			broadcast('task.reordered', {});
			return new Response(null, { status: 204 });
		}
		if (route === 'DELETE /api/tasks') {
			if (searchParams.get('status') !== 'finished') {
				return json({ status: 400, detail: 'pass status=finished' }, 400);
			}
			for (const task of tasks.filter((t) => t.status === 'finished')) remove(task);
			return new Response(null, { status: 204 });
		}
		const taskRoute = /^(GET|POST|DELETE) \/api\/tasks\/(\d+)(?:\/([a-z]+))?$/.exec(route);
		if (taskRoute) {
			const [, method, id, action] = taskRoute;
			const task = tasks.find((t) => t.id === Number(id));
			if (!task) return json({ status: 404, title: 'Not Found', detail: 'no such task' }, 404);
			if (method === 'GET' && !action) return json(detail(task));
			if (method === 'DELETE' && !action) {
				remove(task);
				return new Response(null, { status: 204 });
			}
			if (method === 'POST' && action && isTaskAction(action)) {
				act(task, action);
				return json(task);
			}
		}
		if (route === 'POST /api/tasks') {
			// Untrusted input: check the shape instead of trusting the generated type.
			const body = (await req.json()) as Partial<NewTask> | null;
			if (!body || typeof body.title !== 'string' || !Array.isArray(body.chapters)) {
				return json({ status: 400, detail: 'expected a task' }, 400);
			}
			if (body.chapters.length === 0) {
				return json({ status: 422, detail: 'no chapters to download', field: 'chapters' }, 422);
			}
			return json(createTask(body as NewTask), 201);
		}
		if (route === 'GET /api/series') {
			const info = series(searchParams.get('module') ?? '', searchParams.get('link') ?? '');
			return info
				? json(info)
				: json({ status: 404, title: 'Not Found', detail: 'series not found' }, 404);
		}
		if (route === 'GET /api/favorites') return json(favorites.list());
		if (route === 'POST /api/favorites') {
			// Untrusted input: check the shape instead of trusting the generated type.
			const body = (await req.json()) as { module_id?: unknown; link?: unknown } | null;
			const info =
				typeof body?.module_id === 'string' && typeof body.link === 'string'
					? series(body.module_id, body.link)
					: null;
			if (!info) return json({ status: 404, detail: 'series not found' }, 404);
			const website = modules().find((m) => m.id === info.module_id)?.name ?? info.module_id;
			const { status, body: added } = favorites.add(info, website);
			return json(added, status);
		}
		if (route === 'POST /api/favorites/check') {
			const body = (await req.json().catch(() => ({}))) as { ids?: number[] } | null;
			return favorites.start(body?.ids ?? null, 'new')
				? new Response(null, { status: 202 })
				: json({ status: 409, detail: 'a favorites check is already running' }, 409);
		}
		const favorite =
			/^(PATCH|DELETE) \/api\/favorites\/(\d+)$|^POST \/api\/favorites\/(\d+)\/check-missing$/.exec(
				route
			);
		if (favorite) {
			const id = Number(favorite[2] ?? favorite[3]);
			if (favorite[1] === 'PATCH') {
				const { status, body } = favorites.patch(id, (await req.json()) as FavoritePatch);
				return body ? json(body, status) : new Response(null, { status });
			}
			if (favorite[1] === 'DELETE')
				return new Response(null, { status: favorites.remove(id).status });
			return favorites.start([id], 'missing')
				? new Response(null, { status: 202 })
				: json({ status: 409, detail: 'a favorites check is already running' }, 409);
		}
		if (route === 'POST /api/import') return importUserdata(req, searchParams);
		if (route === 'GET /api/logs') return json(logs);
		if (route === 'GET /api/jobs') return json(jobs);
		if (route === 'GET /api/settings') return json(settings.getSettings());
		if (route === 'PATCH /api/settings') return update(req, settings.patchSettings);
		if (route === 'PATCH /api/settings/all') return update(req, settings.patchAll);
		if (route === 'POST /api/preview-rename') {
			return json(settings.previewRename((await req.json()) as RenamePreviewRequest));
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
		if (route === 'GET /api/accounts') return json(Object.values(accounts));
		const account =
			/^(PUT|DELETE) \/api\/accounts\/([^/]+)$|^POST \/api\/accounts\/([^/]+)\/login$/.exec(route);
		if (account) {
			const entry = accounts[decodeURIComponent(account[2] ?? account[3] ?? '')];
			if (!entry) return new Response(null, { status: 404 });
			if (account[1] === 'PUT') {
				const body = (await req.json()) as AccountRequest;
				const password = passwords[entry.module] ?? '';
				const changed =
					(body.username != null && body.username !== entry.username) ||
					(body.password != null && body.password !== password);
				entry.username = body.username ?? entry.username;
				passwords[entry.module] = body.password ?? password;
				entry.has_password = passwords[entry.module] !== '';
				entry.enabled = body.enabled ?? entry.enabled;
				if (changed) entry.status = 'unknown';
				return json(entry);
			}
			if (account[1] === 'DELETE') {
				Object.assign(entry, {
					enabled: false,
					username: '',
					has_password: false,
					status: 'unknown'
				});
				passwords[entry.module] = '';
				return new Response(null, { status: 204 });
			}
			await new Promise((resolve) => setTimeout(resolve, 600));
			entry.status = entry.username && passwords[entry.module] ? 'valid' : 'invalid';
			return json(entry);
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
			if (job === favoritesJob) {
				const ok = control[2] === 'run' ? favorites.start(null, 'new') : favorites.cancel();
				return ok
					? json(job, 202)
					: json({ status: 409, detail: `job is ${running ? 'already' : 'not'} running` }, 409);
			}
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

		streams.add(emit);

		let ticks = 0;
		const tick = () => {
			ticks++;
			// Waiting tasks start while fewer than two download.
			for (const task of tasks) {
				const running = tasks.filter((t) => t.status === 'downloading').length;
				if (task.status === 'waiting' && running < 2) setStatus(task, 'downloading');
			}
			for (const task of tasks) {
				if (task.status !== 'downloading') continue;
				task.total ||= 20;
				task.done = Math.min(task.done + 1, task.total);
				task.bytes_per_sec = Math.round(1_500_000 + Math.random() * 1_500_000);
				if (task.done === task.total) {
					task.chapters_done++;
					if (task.chapters_done >= task.chapter_count) {
						task.date_last_downloaded = new Date().toISOString();
						setStatus(task, 'finished');
						continue;
					}
					Object.assign(task, { current_chapter: task.chapters_done, done: 0 });
					task.chapters = chapterLabel(task);
				}
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
			const found = favorites.tick(emit);
			if (found) {
				inbox.unshift(found);
				emit('inbox.new', found);
			}
			for (const job of jobs) {
				if (job.state !== 'running' || job === favoritesJob) continue;
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
				streams.delete(emit);
				clearTimeout(opening);
				clearInterval(timer);
			}
		};
		const opening = setTimeout(() => es.onopen?.(new Event('open')), 0);
		const timer = setInterval(tick, 1000);
		return es;
	};

	let logsUrl: string | null = null;
	const logsDownloadUrl = () => {
		if (logsUrl) URL.revokeObjectURL(logsUrl);
		const body = logs.map((line) => JSON.stringify(line)).join('\n') + '\n';
		logsUrl = URL.createObjectURL(new Blob([body], { type: 'application/x-ndjson' }));
		return logsUrl;
	};

	return { fetch, eventSource, taskFilesUrl, logsDownloadUrl };
}
