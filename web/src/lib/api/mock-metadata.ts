import type { MangaBakaStatus, MetadataEvent } from './types';

// The MangaBaka database of the mock backend: absent until downloaded, then a download that
// advances on the mock's event ticks, as fmd-server's `job.metadata.*` events report it.

/** The dump's size, as MangaBaka's server reports it. */
const DUMP_BYTES = 388_000_000;
/** Event ticks a mock download takes. */
const DOWNLOAD_STEPS = 4;
/** The built database's size. */
const DB_BYTES = 214_000_000;

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
}

/** @param downloaded Whether a database is downloaded at first. */
export function createMockMetadata(downloaded = false): MockMetadata {
	let builtAt: string | null = downloaded ? '2026-10-08T22:42:02Z' : null;
	let job: { step: number; cancelled: boolean } | null = null;
	let last: MetadataEvent | null = null;

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
			available: true,
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
		}
	};
}
