import { ApiError, type Api } from '#lib/api/client.ts';
import type { MangaBakaStatus, MetadataEvent } from '#lib/api/types.ts';
import type { EventStore } from '#lib/events.svelte.ts';

const ENDED: MetadataEvent['kind'][] = ['finished', 'cancelled', 'failed'];

/**
 * The MangaBaka database's status and download, as Settings and the setup show them: its progress
 * comes live from the `job.metadata.*` events. Create it while a component initializes; it
 * refreshes the status when a download ends.
 */
export class MangaBakaDownload {
	status = $state<MangaBakaStatus | null>(null);
	busy = $state(false);
	error = $state<string | null>(null);
	/** Why this server cannot download the database, once it said so (503). */
	unavailable = $state<string | null>(null);

	readonly #api: Api;
	readonly #store: EventStore;

	/** The download's last step: live from the event stream, else as the status reported it. */
	readonly event = $derived.by<MetadataEvent | null>(
		() => this.#store.metadata ?? this.status?.progress ?? null
	);
	readonly running = $derived.by(
		() => this.status?.running === true && !(this.event && ENDED.includes(this.event.kind))
	);
	readonly percent = $derived.by(() => {
		const e = this.event;
		return e && e.total > 0 ? Math.min(100, Math.round((e.done / e.total) * 100)) : null;
	});

	constructor(api: Api, store: EventStore) {
		this.#api = api;
		this.#store = store;
		$effect(() => this.refresh());

		// A download that ends changes the date and size.
		let seen: MetadataEvent | null = null;
		$effect(() => {
			const latest = this.#store.metadata;
			if (!latest || latest === seen) return;
			seen = latest;
			if (latest.kind === 'failed') this.error = latest.error ?? 'The download failed.';
			if (ENDED.includes(latest.kind)) this.refresh();
		});
	}

	refresh() {
		this.#api
			.mangabakaStatus()
			.then((s) => (this.status = s))
			.catch(() => (this.error = 'Could not load the MangaBaka database’s status.'));
	}

	async #act(action: () => Promise<void>, failure: string) {
		this.busy = true;
		this.error = null;
		try {
			await action();
		} catch (e) {
			if (e instanceof ApiError && e.status === 503) {
				this.unavailable = e.detail ?? 'it runs without the database job';
			} else this.error = failure;
		} finally {
			this.busy = false;
			this.refresh();
		}
	}

	/** Starts downloading (or updating) the database in the background. */
	download = () =>
		this.#act(() => {
			this.#store.metadata = null;
			return this.#api.downloadMangabaka();
		}, 'Could not start the download.');
	cancel = () => this.#act(() => this.#api.cancelMangabaka(), 'Could not cancel the download.');
	remove = () => this.#act(() => this.#api.removeMangabaka(), 'Could not remove the database.');
}
