<script lang="ts">
	import { ApiError, type Api } from '#lib/api/client.ts';
	import type { ListEvent, ListFailureReason, ListJobKind, ModuleSummary } from '#lib/api/types.ts';
	import type { EventStore } from '#lib/events.svelte.ts';

	let {
		api,
		store,
		module,
		onfinished
	}: {
		api: Api;
		store: EventStore;
		module: ModuleSummary;
		/** Called once a job of the module ends, so the page can reload the list. */
		onfinished: () => void;
	} = $props();

	let busy = $state(false);
	let failure = $state<string | null>(null);

	const event = $derived<ListEvent | undefined>(store.lists[module.id]);
	const running = $derived(
		event ? event.kind === 'started' || event.kind === 'progress' : module.list_job_running
	);
	const JOB: Record<ListJobKind, string> = {
		update: 'Updating the list',
		import_db: 'Getting the list from FMD2-DB'
	};

	/** What a failed job of the module says, by why it failed; the server words the Jobs panel's
	 * "Last error" the same (`crates/fmd-core/src/lists/jobs.rs`). */
	function failedText(reason: ListFailureReason | null | undefined, job: ListJobKind): string {
		const site = module.name;
		switch (reason) {
			case 'no_dump':
				return module.capabilities.update_list
					? `FMD2-DB has no ready-made list for ${site}. Use Update list to build it from the website.`
					: `FMD2-DB has no ready-made list for ${site}, and this website cannot build one itself.`;
			case 'unreachable':
				return `Could not reach FMD2-DB to get the list of ${site}. Check the connection and try again later.`;
			case 'bad_archive':
				return `The list FMD2-DB sent for ${site} is damaged or empty.`;
			default:
				return job === 'update'
					? `Updating the list of ${site} failed.`
					: `Getting the list of ${site} from FMD2-DB failed.`;
		}
	}

	// Reload once a job ends; the event that ended it is the last one until the next job.
	let ended: ListEvent | null = null;
	$effect(() => {
		if (event && !running && event !== ended) {
			ended = event;
			onfinished();
		}
	});

	const updatedText = $derived(
		module.list_updated
			? `updated ${new Date(module.list_updated).toLocaleDateString()}`
			: 'never updated'
	);

	async function start(kind: ListJobKind) {
		busy = true;
		failure = null;
		try {
			await (kind === 'update' ? api.updateList(module.id) : api.importListDb(module.id));
		} catch (e) {
			failure =
				e instanceof ApiError && e.status === 409
					? 'A list job of this website is already running.'
					: e instanceof ApiError && e.status === 503
						? 'List jobs are not available on this server.'
						: 'Could not start the job.';
		} finally {
			busy = false;
		}
	}

	async function cancel() {
		busy = true;
		try {
			await api.cancelListJob(module.id);
		} catch {
			failure = 'Could not cancel the job.';
		} finally {
			busy = false;
		}
	}
</script>

<div class="actions" aria-label="List of {module.name}" role="group">
	<div class="small muted">
		{#if module.list_size}
			<span class="num">{module.list_size.toLocaleString('en')}</span> titles · {updatedText}
		{:else}
			No list yet. Get it from FMD2-DB, or build it from the website (slow).
		{/if}
	</div>
	{#if running}
		<div class="job" role="status">
			<div class="row">
				<span class="grow small">{event ? JOB[event.job] : 'A list job is running'}</span>
				<button class="btn sm" type="button" disabled={busy} onclick={cancel}>Cancel</button>
			</div>
			{#if event && event.total > 0}
				<div
					class="bar"
					role="progressbar"
					aria-label="List job progress"
					aria-valuemin={0}
					aria-valuemax={event.total}
					aria-valuenow={event.done}
				>
					<i style:width="{Math.round((event.done / event.total) * 100)}%"></i>
				</div>
			{/if}
			{#if event?.status_text}
				<span class="small muted status">{event.status_text}</span>
			{/if}
		</div>
	{:else}
		<div class="row">
			<button class="btn sm" type="button" disabled={busy} onclick={() => start('import_db')}
				>Get from FMD2-DB</button
			>
			<button
				class="btn sm"
				type="button"
				disabled={busy || !module.capabilities.update_list}
				onclick={() => start('update')}>Update list</button
			>
		</div>
		{#if event?.kind === 'finished'}
			<p class="small ok" role="status">
				{event.job === 'update'
					? `Added ${event.titles ?? 0} new titles.`
					: `Imported ${event.titles ?? 0} titles.`}
			</p>
		{:else if event?.kind === 'failed'}
			<div class="failed small" role="alert">
				<p class="bad">{failedText(event.reason, event.job)}</p>
				{#if event.reason === 'no_dump' && module.capabilities.update_list}
					<div class="row">
						<button class="btn sm" type="button" disabled={busy} onclick={() => start('update')}
							>Update list</button
						>
					</div>
				{/if}
				{#if event.error}
					<details>
						<summary class="muted">Details</summary>
						<pre class="mono">{event.error}</pre>
					</details>
				{/if}
			</div>
		{:else if event?.kind === 'cancelled'}
			<p class="small muted" role="status">Cancelled.</p>
		{/if}
	{/if}
	{#if failure}
		<p class="small bad" role="alert">{failure}</p>
	{/if}
</div>

<style>
	.actions,
	.job,
	.failed {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.row {
		display: flex;
		gap: var(--sp-2);
		align-items: center;
		flex-wrap: wrap;
	}
	.grow {
		flex: 1;
		min-width: 0;
	}
	.status {
		overflow-wrap: anywhere;
	}
	p {
		margin: 0;
	}
	summary {
		cursor: pointer;
	}
	pre {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		margin: var(--sp-1) 0 0;
	}
	.ok {
		color: var(--ok);
	}
	.bad {
		color: var(--bad);
	}
</style>
