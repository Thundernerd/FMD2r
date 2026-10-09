<script lang="ts">
	import type { Api } from '#lib/api/client.ts';
	import type { MangaBakaStatus, MetadataEvent } from '#lib/api/types.ts';
	import type { EventStore } from '#lib/events.svelte.ts';

	let { api, store }: { api: Api; store: EventStore } = $props();

	let status = $state<MangaBakaStatus | null>(null);
	let busy = $state(false);
	let error = $state<string | null>(null);

	function refresh() {
		api
			.mangabakaStatus()
			.then((s) => (status = s))
			.catch(() => (error = 'Could not load the MangaBaka database’s status.'));
	}
	$effect(refresh);

	/** The download's last step: live from the event stream, else as the status reported it. */
	const event = $derived<MetadataEvent | null>(store.metadata ?? status?.progress ?? null);
	const running = $derived(
		status?.running === true && !(event && ['finished', 'cancelled', 'failed'].includes(event.kind))
	);
	const percent = $derived(
		event && event.total > 0 ? Math.min(100, Math.round((event.done / event.total) * 100)) : null
	);

	// A download that ends changes the date and size.
	let seen: MetadataEvent | null = null;
	$effect(() => {
		const latest = store.metadata;
		if (!latest || latest === seen) return;
		seen = latest;
		if (latest.kind === 'failed') error = latest.error ?? 'The download failed.';
		if (['finished', 'cancelled', 'failed'].includes(latest.kind)) refresh();
	});

	async function act(action: () => Promise<void>, failure: string) {
		busy = true;
		error = null;
		try {
			await action();
		} catch {
			error = failure;
		} finally {
			busy = false;
			refresh();
		}
	}

	const download = () =>
		act(() => {
			store.metadata = null;
			return api.downloadMangabaka();
		}, 'Could not start the download.');
	const cancel = () => act(() => api.cancelMangabaka(), 'Could not cancel the download.');
	const remove = () => act(() => api.removeMangabaka(), 'Could not remove the database.');

	const megabytes = (bytes: number) => `${Math.round(bytes / 1_000_000).toLocaleString('en')} MB`;
	const date = (iso: string) =>
		new Date(iso).toLocaleString('en', { dateStyle: 'medium', timeStyle: 'short' });
</script>

<div class="panel">
	<p class="small muted">
		A local copy of <a href="https://mangabaka.org" target="_blank" rel="noreferrer">MangaBaka</a>’s
		database gives list titles a format, a publication status and descriptions, matched offline: no
		title leaves this server. Nothing is downloaded until you ask; the download is about 390 MB.
	</p>
	{#if status}
		{#if status.downloaded && status.built_at}
			<p class="state">
				<span>Built {date(status.built_at)}</span>
				{#if status.bytes != null}<span class="muted">· {megabytes(status.bytes)}</span>{/if}
				{#if status.next_refresh}
					<span class="muted small">· next update {date(status.next_refresh)}</span>
				{/if}
			</p>
		{:else}
			<p class="state muted">Not downloaded.</p>
		{/if}

		{#if running}
			<div class="progress">
				<div
					class="bar"
					role="progressbar"
					aria-label="Download progress"
					aria-valuemin="0"
					aria-valuemax="100"
					aria-valuenow={percent ?? undefined}
				>
					<i style:width="{percent ?? 0}%"></i>
				</div>
				<span class="small muted">
					{event?.phase === 'matching' ? 'Matching the lists…' : event?.status_text || 'Starting…'}
				</span>
			</div>
		{/if}

		<div class="actions">
			{#if running}
				<button class="btn" type="button" disabled={busy} onclick={cancel}>Cancel</button>
			{:else if status.downloaded}
				<button class="btn" type="button" disabled={busy || !status.available} onclick={download}
					>Update</button
				>
				<button class="btn" type="button" disabled={busy} onclick={remove}>Remove</button>
			{:else}
				<button
					class="btn primary"
					type="button"
					disabled={busy || !status.available}
					onclick={download}>Download</button
				>
			{/if}
		</div>
		{#if !status.available}
			<p class="small muted">This server cannot download it.</p>
		{/if}
	{/if}
	{#if error}
		<p class="bad small" role="alert">{error}</p>
	{/if}
</div>

<style>
	.panel {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
		padding-top: var(--sp-3);
		border-top: 1px solid var(--line);
	}
	p {
		margin: 0;
	}
	.state {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-1);
		align-items: baseline;
	}
	.progress {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
	}
	.actions {
		display: flex;
		gap: var(--sp-2);
	}
	.bad {
		color: var(--bad);
	}
</style>
