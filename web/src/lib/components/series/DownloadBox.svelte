<script lang="ts">
	import type { Api } from '#lib/api/client.ts';
	import { ApiError, ValidationError } from '#lib/api/client.ts';
	import type { Destination, OutputFormat, SeriesInfo, TaskSummary } from '#lib/api/types.ts';
	import DestinationPicker from '#lib/components/destinations/DestinationPicker.svelte';

	let {
		api,
		series,
		selected,
		saveTo = $bindable(),
		destinations,
		format,
		onqueued
	}: {
		api: Api;
		series: SeriesInfo;
		selected: Set<number>;
		/** The folder picked: a destination's or a custom one. */
		saveTo: string;
		/** The configured destinations; empty until the settings are loaded. */
		destinations: Destination[];
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

	const uid = $props.id();
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
	const seen = $derived(picked.filter((c) => c.downloaded).length);

	/** The folder the chapters go to, the manga folder included, as the server resolves it. */
	let folder = $state<string | null>(null);
	$effect(() => {
		const request = {
			module_id: series.module_id,
			title: series.title,
			authors: series.authors,
			artists: series.artists,
			save_to: saveTo
		};
		if (!saveTo.trim()) {
			folder = null;
			return;
		}
		const timer = setTimeout(() => {
			api
				.saveFolder(request)
				.then((f) => (folder = f))
				.catch(() => (folder = null));
		}, 250);
		return () => clearTimeout(timer);
	});

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
			{#if seen}
				<span class="small muted">· {seen} seen before</span>
			{/if}
		{/if}
	</p>
	<div class="fields">
		<div class="field grow">
			<label class="label" for="{uid}-save-to">Save to</label>
			<DestinationPicker id="{uid}-save-to" label="Save to" {destinations} bind:value={saveTo} />
			{#if folder}
				<span class="small muted folder">In <span class="mono">{folder}</span></span>
			{/if}
		</div>
		<div class="field">
			<span class="label">Format</span>
			<span class="format">
				<b>{format ? FORMATS[format] : '…'}</b>
				<a class="small" href="/settings#section-output">Change</a>
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
	.folder {
		word-break: break-all;
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
