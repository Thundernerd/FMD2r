<script lang="ts" module>
	/** What runs: a dry run (`check`), the import, or nothing. */
	export type ImportBusy = 'check' | 'import' | null;
</script>

<script lang="ts">
	import type { Api } from '#lib/api/client.ts';
	import { ApiError } from '#lib/api/client.ts';
	import type { ImportReport, JobState } from '#lib/api/types.ts';
	import { SOURCES, alreadyExisting, invalid, pathMaps } from '#lib/library/import.ts';

	let {
		api,
		job,
		onimported,
		busy = $bindable(null)
	}: {
		api: Api;
		/** The `import` job, for its progress. */
		job: JobState | undefined;
		/** An import (not a dry run) finished. */
		onimported: () => void;
		busy?: ImportBusy;
	} = $props();

	let file = $state<File | null>(null);
	let maps = $state('');
	let timezone = $state('');
	let resume = $state(false);
	let report = $state<ImportReport | null>(null);
	let error = $state<string | null>(null);

	/** IANA zones the browser knows; the server's own zone is the default. */
	const zones = Intl.supportedValuesOf('timeZone');
	const browserZone = Intl.DateTimeFormat().resolvedOptions().timeZone;

	/** The file and options of the last dry run; importing needs a dry run of exactly these. */
	let checked = $state<{ file: File; options: string } | null>(null);
	const options = $derived(JSON.stringify([maps, timezone, resume]));
	const canImport = $derived(
		busy === null &&
			report !== null &&
			report.dry_run &&
			checked?.file === file &&
			checked.options === options
	);
	const skipped = $derived.by(() => {
		const r = report;
		return r ? SOURCES.flatMap(({ key: k }) => invalid(r[k])) : [];
	});
	const progress = $derived(busy !== null && job?.state === 'running' ? job : null);

	function message(e: unknown): string {
		if (!(e instanceof ApiError)) return 'Could not reach the server.';
		if (e.status === 409) return 'An import is already running.';
		if (e.status === 413) return e.detail ?? 'The zip is too large.';
		return e.detail ?? 'The import failed.';
	}

	async function run(dryRun: boolean) {
		if (!file) return;
		busy = dryRun ? 'check' : 'import';
		error = null;
		const ran = { file, options };
		try {
			report = await api.importFmd2(file, {
				dry_run: dryRun,
				resume,
				map_path: pathMaps(maps),
				...(timezone ? { timezone } : {})
			});
			checked = dryRun ? ran : null;
			if (!dryRun) onimported();
		} catch (e) {
			// A failed import keeps its dry run's report, to import again from.
			if (dryRun) report = null;
			error = message(e);
		} finally {
			busy = null;
		}
	}
</script>

<div class="flow">
	<p class="small muted">
		Zip FMD2's <span class="mono">userdata</span> folder and upload it here. Check it first: a dry run
		reports what would be imported and writes nothing.
	</p>

	<label class="field">
		<span class="label">FMD2 userdata folder, zipped</span>
		<input
			class="input"
			type="file"
			accept=".zip,application/zip"
			onchange={(e) => (file = e.currentTarget.files?.[0] ?? null)}
		/>
	</label>
	<label class="field">
		<span class="label">Path maps</span>
		<textarea class="input mono" rows="2" placeholder="C:\Manga=/data/manga" bind:value={maps}
		></textarea>
		<span class="small muted">One FROM=TO per line: rewrites FMD2's save-to folders.</span>
	</label>
	<label class="field">
		<span class="label">FMD2's time zone</span>
		<select class="input" bind:value={timezone}>
			<option value="">The server's time zone</option>
			<option value={browserZone}>This browser's ({browserZone})</option>
			{#each zones as zone (zone)}
				<option value={zone}>{zone}</option>
			{/each}
		</select>
		<span class="small muted"
			>FMD2 stores the times it added and checked things without a zone.</span
		>
	</label>
	<label class="check">
		<input type="checkbox" bind:checked={resume} />
		Resume the downloads FMD2 was running
	</label>

	<div class="actions">
		<button class="btn" type="button" disabled={!file || busy !== null} onclick={() => run(true)}
			>{busy === 'check' ? 'Checking…' : 'Check'}</button
		>
		<button class="btn primary" type="button" disabled={!canImport} onclick={() => run(false)}
			>{busy === 'import' ? 'Importing…' : 'Import'}</button
		>
		{#if progress}
			<span class="small muted num">{progress.done}/{progress.total}</span>
		{/if}
	</div>

	{#if error}
		<p class="bad" role="alert">{error}</p>
	{/if}

	{#if report}
		<p role="status" class="small">
			{report.dry_run ? 'Dry run: nothing was written.' : 'Imported.'}
		</p>
		<table class="report" aria-label="Import report">
			<thead>
				<tr>
					<th scope="col">Source</th>
					<th scope="col" class="n">{report.dry_run ? 'To import' : 'Imported'}</th>
					<th scope="col" class="n">Already there</th>
					<th scope="col" class="n">Invalid</th>
				</tr>
			</thead>
			<tbody>
				{#each SOURCES as { key: k, label } (k)}
					{@const source = report[k]}
					<tr>
						<th scope="row">{label}</th>
						{#if source.found}
							<td class="n num">{source.imported}</td>
							<td class="n num">{alreadyExisting(source)}</td>
							<td class="n num">{invalid(source).length}</td>
						{:else}
							<td colspan="3" class="muted">not found</td>
						{/if}
					</tr>
				{/each}
			</tbody>
		</table>
		{#if skipped.length > 0}
			<ul class="notes small">
				{#each skipped as s, i (i)}
					<li>Skipped <span class="mono">{s.item}</span>: {s.why}</li>
				{/each}
			</ul>
		{/if}
		{#if report.warnings.length > 0}
			<ul class="notes small warn">
				{#each report.warnings as w, i (i)}
					<li>{w}</li>
				{/each}
			</ul>
		{/if}
		{#if report.unmapped.length > 0}
			<details class="small">
				<summary>{report.unmapped.length} FMD2 values have no FMD2r counterpart</summary>
				<ul class="notes">
					{#each report.unmapped as u, i (i)}
						<li><span class="mono">{u.source} {u.key}</span> = {u.value}</li>
					{/each}
				</ul>
			</details>
		{/if}
	{/if}
</div>

<style>
	.flow {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
	}
	p {
		margin: 0;
	}
	.field {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
	}
	.check {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
	}
	.actions {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
	}
	.report {
		border-collapse: collapse;
		width: 100%;
		font-size: var(--fs-sm);
	}
	.report th,
	.report td {
		text-align: left;
		padding: 4px 6px;
		border-bottom: 1px solid var(--line);
	}
	.report thead th {
		color: var(--muted);
		font-weight: 600;
	}
	.report tbody th {
		font-weight: 400;
	}
	.report .n {
		text-align: right;
	}
	.notes {
		margin: 0;
		padding-left: var(--sp-4);
	}
	.warn {
		color: var(--warn);
	}
	.bad {
		color: var(--bad);
	}
</style>
