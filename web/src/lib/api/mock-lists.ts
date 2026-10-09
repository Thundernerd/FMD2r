import type {
	FacetValue,
	ListEvent,
	ListFacets,
	ListItem,
	ListJobKind,
	ModuleSummary,
	SearchPage
} from './types';

// The manga lists of the mock backend: generated titles per module, searched and filtered the
// way fmd-server does it (`MasterListRepo::search`), and list jobs that advance on the mock's
// event ticks.

const GENRES = [
	'Action',
	'Adventure',
	'Comedy',
	'Drama',
	'Fantasy',
	'Horror',
	'Mystery',
	'Romance',
	'School Life',
	'Shounen',
	'Slice of Life',
	'Sports'
];
const FIRST = ['Blade', 'Dragon', 'Witch', 'Shadow', 'Star', 'Moon', 'Demon', 'Iron', 'Ghost'];
const SECOND = ['Academy', 'Diaries', 'Saga', 'Hunter', 'Tower', 'Kingdom', 'Atelier', 'Days'];
const AUTHORS = ['Kamome Shirahama', 'Ryoko Kui', 'Yuto Suzuki', 'Makoto Yukimura', 'Sing Shong'];

/** How many titles each module lists at first; a module missing here has no list yet. */
const LIST_SIZES: Record<string, number> = {
	mangadex: 140,
	comick: 90,
	webtoons: 48,
	rawkuma: 30,
	tmo: 24
};
/** Titles a "Get from FMD2-DB" import gives a module. */
const DB_SIZE = 60;
/** Directory pages a mock update walks, one per event tick. */
const UPDATE_PAGES = 8;
const PAGE_SIZE = 50;

/** Julian day number of 2026-10-08 (`DateToJDN`). */
const TODAY_JDN = 2_461_322;

function title(module: string, i: number): ListItem {
	const seed = [...module].reduce((h, c) => h + c.charCodeAt(0), 0) + i * 7;
	const pick = <T>(list: T[], n: number): T => list[n % list.length] as T;
	const genres = [pick(GENRES, seed), pick(GENRES, seed * 3 + 1), pick(GENRES, seed * 5 + 4)];
	return {
		module_id: module,
		link: `/manga/${module}-${i}`,
		title: `${pick(FIRST, seed)} ${pick(SECOND, seed >> 1)} ${i + 1}`,
		alttitles: '',
		authors: pick(AUTHORS, seed),
		artists: '',
		genres: [...new Set(genres)],
		status: String(seed % 4),
		numchapter: 1 + (seed % 200),
		added_jdn: TODAY_JDN - (i % 40)
	};
}

const list = (module: string, size: number, from = 0): ListItem[] =>
	Array.from({ length: size }, (_, i) => title(module, from + i));

interface Job {
	kind: ListJobKind;
	done: number;
	total: number;
	cancelled: boolean;
}

export interface MockLists {
	/** The list fields of `module`'s `GET /api/modules` entry. */
	summary(module: string): Pick<ModuleSummary, 'list_size' | 'list_updated' | 'list_job_running'>;
	search(params: URLSearchParams): SearchPage;
	facets(params: URLSearchParams): ListFacets;
	/** Starts a job; `false` when one of `module` already runs. */
	start(module: string, kind: ListJobKind): boolean;
	/** Asks the job of `module` to stop; `false` when none runs. */
	cancel(module: string): boolean;
	/** Advances every running job one step, reporting each event. */
	tick(emit: (event: ListEvent) => void): void;
}

export function createMockLists(): MockLists {
	const lists = new Map<string, ListItem[]>(
		Object.entries(LIST_SIZES).map(([module, size]) => [module, list(module, size)])
	);
	const updated = new Map<string, string>(
		[...lists.keys()].map((module) => [module, '2026-10-07T06:00:00Z'])
	);
	const jobs = new Map<string, Job>();

	/** The titles `params` selects, by title, like `MasterListRepo::search`. */
	const matching = (params: URLSearchParams, withFilters: boolean): ListItem[] => {
		const module = params.get('module');
		const words = (params.get('q') ?? '').toLowerCase().split(/\s+/).filter(Boolean);
		const split = (name: string) => (params.get(name) ?? '').split(',').filter(Boolean);
		const include = withFilters ? split('genres_include') : [];
		const exclude = withFilters ? split('genres_exclude') : [];
		const status = withFilters ? params.get('status') : null;
		const items = module ? (lists.get(module) ?? []) : [...lists.values()].flat();
		return items
			.filter((item) => {
				const titleWords = `${item.title} ${item.alttitles}`.toLowerCase().split(/\s+/);
				const genres = item.genres.join(', ');
				return (
					words.every((w) => titleWords.some((t) => t.startsWith(w))) &&
					include.every((g) => genres.includes(g)) &&
					!exclude.some((g) => genres.includes(g)) &&
					(!status || item.status === status)
				);
			})
			.sort((a, b) => a.title.localeCompare(b.title, 'en', { sensitivity: 'base' }));
	};

	const counts = (values: string[]): FacetValue[] => {
		const map = new Map<string, number>();
		for (const value of values) map.set(value, (map.get(value) ?? 0) + 1);
		return [...map]
			.map(([value, count]) => ({ value, count }))
			.sort((a, b) => b.count - a.count || a.value.localeCompare(b.value));
	};

	const event = (module: string, job: Job, kind: ListEvent['kind']): ListEvent => ({
		module_id: module,
		job: job.kind,
		kind,
		status_text:
			kind !== 'progress'
				? ''
				: job.kind === 'update'
					? `Looking for new title(s) ${job.done}/${job.total}...`
					: 'Downloading...',
		done: job.done,
		total: job.total,
		titles: null,
		error: null,
		reason: null
	});

	return {
		summary: (module) => ({
			list_size: lists.get(module)?.length ?? 0,
			list_updated: updated.get(module) ?? null,
			list_job_running: jobs.has(module)
		}),

		search(params) {
			const items = matching(params, true);
			const page = Math.max(1, Number(params.get('page') ?? 1) || 1);
			const pageSize = Math.min(200, Math.max(1, Number(params.get('page_size') ?? PAGE_SIZE)));
			return {
				items: items.slice((page - 1) * pageSize, page * pageSize),
				total: items.length,
				page,
				page_size: pageSize
			};
		},

		facets(params) {
			const items = matching(params, false);
			return {
				genres: counts(items.flatMap((i) => i.genres)),
				statuses: counts(items.map((i) => i.status))
			};
		},

		start(module, kind) {
			if (jobs.has(module)) return false;
			jobs.set(module, {
				kind,
				done: 0,
				total: kind === 'update' ? UPDATE_PAGES : 3,
				cancelled: false
			});
			return true;
		},

		cancel(module) {
			const job = jobs.get(module);
			if (!job) return false;
			job.cancelled = true;
			return true;
		},

		tick(emit) {
			for (const [module, job] of jobs) {
				if (job.cancelled) {
					jobs.delete(module);
					emit({ ...event(module, job, 'cancelled'), titles: 0 });
					continue;
				}
				if (job.done === 0) emit(event(module, job, 'started'));
				job.done++;
				emit(event(module, job, 'progress'));
				if (job.done < job.total) continue;
				jobs.delete(module);
				// FMD2-DB only has dumps of the modules that start with a list.
				if (job.kind === 'import_db' && !(module in LIST_SIZES)) {
					const url = `https://raw.githubusercontent.com/dazedcat19/FMD2-DB/master/7z/${module}.7z`;
					emit({
						...event(module, job, 'failed'),
						error: `${module}: downloading ${url} failed with HTTP status 404`,
						reason: 'no_dump'
					});
					continue;
				}
				const current = lists.get(module) ?? [];
				const added =
					job.kind === 'update' ? list(module, 5, current.length) : list(module, DB_SIZE);
				lists.set(module, job.kind === 'update' ? [...current, ...added] : added);
				updated.set(module, new Date().toISOString());
				emit({ ...event(module, job, 'finished'), titles: added.length });
			}
		}
	};
}
