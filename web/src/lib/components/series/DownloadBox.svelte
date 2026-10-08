<script lang="ts">
	import type { Api } from '#lib/api/client.ts';
	import { ApiError, ValidationError } from '#lib/api/client.ts';
	import type { OutputFormat, SeriesInfo, TaskProgress } from '#lib/api/types.ts';

	let {
		api,
		series,
		selected,
		saveTo = $bindable(),
		format = $bindable(),
		onqueued
	}: {
		api: Api;
		series: SeriesInfo;
		selected: Set<number>;
		saveTo: string;
		format: OutputFormat;
		onqueued: (task: TaskProgress) => void;
	} = $props();

	const FORMATS: { value: OutputFormat; label: string }[] = [
		{ value: 'folder', label: 'Folder' },
		{ value: 'zip', label: 'ZIP' },
		{ value: 'cbz', label: 'CBZ' },
		{ value: 'pdf', label: 'PDF' },
		{ value: 'epub', label: 'EPUB' }
	];

	let busy = $state(false);
	let queued = $state<TaskProgress | null>(null);
	let error = $state<string | null>(null);

	/** The selected chapters in module order. */
	const picked = $derived(
		[...selected]
			.sort((a, b) => a - b)
			.map((i) => series.chapters[i])
			.filter((c) => c !== undefined)
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
				chapters: picked.map(({ name, link }) => ({ name, link })),
				save_to: saveTo,
				format
			});
			queued = task;
			onqueued(task);
		} catch (e) {
			error =
				e instanceof ValidationError
					? e.detail
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
		<label class="field">
			<span class="label">Format</span>
			<select class="input" bind:value={format}>
				{#each FORMATS as f (f.value)}
					<option value={f.value}>{f.label}</option>
				{/each}
			</select>
		</label>
	</div>
	<div class="actions">
		<button class="btn primary" type="button" disabled={count === 0 || busy} onclick={download}>
			{count === 0 ? 'Download' : `Download ${count} ${noun}`}
		</button>
	</div>
	<div role="status" class="small">
		{#if queued}
			Queued: <b>{queued.title}</b>
			{queued.chapters}. <a href="/queue">Open queue</a>
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
