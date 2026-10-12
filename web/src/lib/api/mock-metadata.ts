import type { MangaBakaSearchResult, MangaBakaStatus, MetadataEvent, SeriesLink } from './types';

// The MangaBaka database of the mock backend: absent until downloaded, then a download that
// advances on the mock's event ticks, as fmd-server's `job.metadata.*` events report it.

/** The dump's size, as MangaBaka's server reports it. */
const DUMP_BYTES = 388_000_000;
/** Event ticks a mock download takes. */
const DOWNLOAD_STEPS = 4;
/** The built database's size. */
const DB_BYTES = 214_000_000;

/** The MangaBaka series the mock database holds. */
const CATALOG: MangaBakaSearchResult[] = [
	{
		id: 377,
		title: 'One Piece',
		english_title: 'One Piece',
		year: 1997,
		format: 'manga',
		cover: null
	},
	{
		id: 9314,
		title: 'Sakamoto Days',
		english_title: 'Sakamoto Days',
		year: 2020,
		format: 'manga',
		cover: null
	},
	{
		id: 1288,
		title: 'Sakamoto desu ga?',
		english_title: "Haven't You Heard? I'm Sakamoto",
		year: 2011,
		format: 'manga',
		cover: null
	},
	{
		id: 8153,
		title: 'Sousou no Frieren',
		english_title: "Frieren: Beyond Journey's End",
		year: 2020,
		format: 'manga',
		cover: null
	}
];

/** The search key of a title: lower-cased words. */
const key = (s: string) =>
	s
		.toLowerCase()
		.replace(/[^\p{L}\p{N}]+/gu, ' ')
		.trim();

export interface MockMetadata {
	status(): MangaBakaStatus;
	/** Starts a download; `false` when one runs. */
	start(): boolean;
	/** Asks the download to stop; `false` when none runs. */
	cancel(): boolean;
	/** Deletes the database; `false` while a download runs. */
	remove(): boolean;
	/** Whether a database is downloaded, so list titles carry MangaBaka metadata. */
	downloaded(): boolean;
	/** Advances the running download one step, reporting each event. */
	tick(emit: (event: MetadataEvent) => void): void;
	/** `GET /api/metadata/search`: by the beginning of a title, an ID or a MangaBaka URL. */
	search(query: string): MangaBakaSearchResult[];
	/** A series of the database. */
	series(id: number): MangaBakaSearchResult | undefined;
	/** A series' MangaBaka links, as `SeriesInfo.mangabaka.links`. */
	links(id: number): SeriesLink[];
	/** The series picked by hand for a series, by module and link. */
	picked(module: string, link: string): number | undefined;
	/** Picks a series by hand; `false` when the database has no such series. */
	pick(module: string, link: string, id: number): boolean;
	unpick(module: string, link: string): void;
}

/**
 * @param downloaded Whether a database is downloaded at first.
 * @param available Whether the server can use MangaBaka at all.
 */
export function createMockMetadata(downloaded = false, available = true): MockMetadata {
	let builtAt: string | null = downloaded ? '2026-10-08T22:42:02Z' : null;
	let job: { step: number; cancelled: boolean } | null = null;
	let last: MetadataEvent | null = null;
	const picks = new Map<string, number>();
	const pickKey = (module: string, link: string) => `${module}\n${link}`;

	const event = (kind: MetadataEvent['kind'], step: number): MetadataEvent => ({
		kind,
		phase: step < DOWNLOAD_STEPS ? 'downloading' : 'matching',
		status_text: kind === 'progress' ? 'Downloading and building...' : '',
		done: Math.round((DUMP_BYTES * step) / DOWNLOAD_STEPS),
		total: DUMP_BYTES,
		error: null
	});

	return {
		status: () => ({
			available,
			downloaded: builtAt !== null,
			built_at: builtAt,
			bytes: builtAt ? DB_BYTES : null,
			running: job !== null,
			progress: job ? last : null,
			next_refresh: builtAt
				? new Date(Date.parse(builtAt) + 7 * 24 * 3600 * 1000).toISOString()
				: null
		}),

		start() {
			if (job) return false;
			job = { step: 0, cancelled: false };
			return true;
		},

		cancel() {
			if (!job) return false;
			job.cancelled = true;
			return true;
		},

		remove() {
			if (job) return false;
			builtAt = null;
			return true;
		},

		downloaded: () => builtAt !== null,

		tick(emit) {
			if (!job) return;
			if (job.cancelled) {
				job = null;
				emit(event('cancelled', 0));
				return;
			}
			if (job.step === 0) emit((last = event('started', 0)));
			job.step++;
			if (job.step < DOWNLOAD_STEPS) {
				emit((last = event('progress', job.step)));
				return;
			}
			job = null;
			builtAt = new Date().toISOString();
			emit(event('finished', DOWNLOAD_STEPS));
		},

		search(query) {
			const q = query.trim();
			const id =
				/^(?:https?:\/\/)?(?:www\.)?mangabaka\.org\/(\d+)/i.exec(q)?.[1] ?? /^\d+$/.exec(q)?.[0];
			if (id) return CATALOG.filter((s) => s.id === Number(id));
			const k = key(q);
			if (!k) return [];
			return CATALOG.filter((s) =>
				[s.title, s.english_title ?? ''].some((t) => key(t).startsWith(k))
			);
		},

		series: (id) => CATALOG.find((s) => s.id === id),

		links: (id) => [{ label: 'MangaBaka', url: `https://mangabaka.org/${id}` }],

		picked: (module, link) => picks.get(pickKey(module, link)),

		pick(module, link, id) {
			if (!CATALOG.some((s) => s.id === id)) return false;
			picks.set(pickKey(module, link), id);
			return true;
		},

		unpick(module, link) {
			picks.delete(pickKey(module, link));
		}
	};
}
