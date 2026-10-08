<script lang="ts">
	import type { Api } from '#lib/api/client.ts';
	import { ApiError, ValidationError } from '#lib/api/client.ts';
	import type { OutputFormat, SeriesInfo, TaskSummary } from '#lib/api/types.ts';

	let {
		api,
		series,
		selected,
		saveTo = $bindable(),
		format,
		onqueued
	}: {
		api: Api;
		series: SeriesInfo;
		selected: Set<number>;
		saveTo: string;
		/** The configured output format; `null` until the settings are loaded. */
		format: OutputFormat | null;
		onqueued: (task: TaskSummary) => void;
	} = $props();

	// Like FMD2, every download is packed in the one configured format (`rgOptionCompress`).
	const FORMATS: Record<OutputFormat, string> = {
		folder: 'Folder',
		zip: 'ZIP',
		cbz: 'CBZ',
		pdf: 'PDF',
		epub: 'EPUB'
	};

	let busy = $state(false);
	let queued = $state<TaskSummary | null>(null);
	let error = $state<string | null>(null);

	/** The selected chapters in module order, with their 1-based number in the series. */
	const picked = $derived(
		[...selected]
			.sort((a, b) => a - b)
			.flatMap((i) => {
				const chapter = series.chapters[i];
				return chapter ? [{ ...chapter, number: i + 1 }] : [];
			})
	);
	const count = $derived(picked.length);
	const noun = $derived(count === 1 ? 'chapter' : 'chapters');
	const already = $derived(picked.filter((c) => c.downloaded).length);

	async function download() {
		if (count === 0 || busy) return;
		busy = true;
		error = null;
		queued = null;
		try {
			const task = await api.createTask({
				module_id: series.module_id,
				link: series.link,
				title: series.title,
				authors: series.authors,
				artists: series.artists,
				chapters: picked.map(({ name, link, number }) => ({ name, link, number })),
				save_to: saveTo
			});
			queued = task;
			onqueued(task);
		} catch (e) {
			error =
				e instanceof ValidationError
					? e.detail
					: e instanceof ApiError && [404, 405, 501].includes(e.status)
						? 'This FMD2r build cannot queue downloads yet.'
						: e instanceof ApiError
							? `The download could not be queued (HTTP ${e.status}).`
							: 'Could not reach FMD2r. Try again.';
		} finally {
			busy = false;
		}
	}
</script>

<section class="box" aria-label="Download">
	<p class="summary">
		{#if count === 0}
			<span class="muted">No chapters selected</span>
		{:else}
			<b class="num">{count} {noun} selected</b>
			{#if already}
				<span class="small muted">· {already} downloaded before</span>
			{/if}
		{/if}
	</p>
	<div class="fields">
		<label class="field grow">
			<span class="label">Save to</span>
			<input class="input mono" type="text" bind:value={saveTo} />
		</label>
		<div class="field">
			<span class="label">Format</span>
			<span class="format">
				<b>{format ? FORMATS[format] : '…'}</b>
				<a class="small" href="/settings#output">Change</a>
			</span>
		</div>
	</div>
	<div class="actions">
		<button class="btn primary" type="button" disabled={count === 0 || busy} onclick={download}>
			{count === 0 ? 'Download' : `Download ${count} ${noun}`}
		</button>
	</div>
	<div role="status" class="small">
		{#if queued}
			Queued: <b>{queued.title}</b>, {queued.chapter_count}
			{queued.chapter_count === 1 ? 'chapter' : 'chapters'}. <a href="/queue">Open queue</a>
		{/if}
	</div>
	{#if error}
		<p class="error small" role="alert">{error}</p>
	{/if}
</section>

<style>
	.box {
		display: flex;
		flex-direction: column;
		gap: 10px;
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		padding: 12px 14px;
	}
	.summary {
		margin: 0;
	}
	.fields {
		display: flex;
		gap: var(--sp-3);
		flex-wrap: wrap;
	}
	.field {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
	}
	.format {
		display: flex;
		gap: var(--sp-2);
		align-items: baseline;
		padding: 7px 0;
	}
	.field.grow {
		flex: 1 1 260px;
		min-width: 0;
	}
	.actions {
		display: flex;
		gap: var(--sp-2);
	}
	.btn:disabled {
		opacity: 0.55;
		cursor: default;
	}
	.error {
		margin: 0;
		padding: 6px 10px;
		border-radius: var(--r);
		background: var(--bad-soft);
		color: var(--bad);
	}
</style>
