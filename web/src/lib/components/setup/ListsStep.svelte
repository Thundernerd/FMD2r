<script lang="ts">
	import { ApiError } from '#lib/api/client.ts';
	import type { ListEvent, ModuleSummary } from '#lib/api/types.ts';
	import { listFailedText, listUpdatedText } from '#lib/discover/list-jobs.ts';
	import type { StepProps } from '#lib/setup/steps.ts';

	let { api, store, settings }: StepProps = $props();

	/** The websites selected in the step before, in the order the server lists them. */
	let websites = $state<ModuleSummary[] | null>(null);
	let loadFailed = $state(false);
	/** The websites to download a ready-made list for; at first, the ones without a list. */
	let checked = $state<Record<string, boolean>>({});

	function load() {
		loadFailed = false;
		api
			.listModules()
			.then((modules) => {
				const ids = new Set(settings.general.selected_websites);
				// A module can be listed once per website it serves; it has one list.
				const selected = modules.filter(
					(m, i) => ids.has(m.id) && modules.findIndex((o) => o.id === m.id) === i
				);
				checked = Object.fromEntries(selected.map((m) => [m.id, m.list_size === 0]));
				websites = selected;
			})
			.catch(() => (loadFailed = true));
	}
	$effect(load);

	/**
	 * The websites whose download this step started, or found running already, with the event
	 * of their list as it was then: until a newer one arrives, that one is of an earlier job.
	 */
	let started = $state<Record<string, { before: ListEvent | undefined }>>({});
	/** Why a website's download did not start. */
	let startFailures = $state<Record<string, string>>({});
	let busy = $state(false);
	/** Whether the server can't run list jobs (503), so only Discover's later attempt is left. */
	let unavailable = $state(false);

	/** The latest event of the list of `module`, unless it is of a job before the one started here. */
	function currentEvent(module: ModuleSummary): ListEvent | undefined {
		const event = store.lists[module.id];
		return started[module.id] && event === started[module.id]?.before ? undefined : event;
	}

	/** Whether a list job of `module` runs, by its latest event as `ListActions` has it. */
	function running(module: ModuleSummary): boolean {
		const event = currentEvent(module);
		return event
			? event.kind === 'started' || event.kind === 'progress'
			: module.list_job_running || started[module.id] !== undefined;
	}

	export function nextLabel(): string | undefined {
		const downloading = Object.keys(started).length > 0 || (websites ?? []).some((m) => running(m));
		return downloading ? undefined : 'Skip';
	}

	/** Starts the ready-made list download of every checked website; each runs on its own. */
	async function download() {
		busy = true;
		await Promise.all(
			(websites ?? [])
				.filter((m) => checked[m.id])
				.map(async (m) => {
					delete startFailures[m.id];
					const before = store.lists[m.id];
					try {
						await api.importListDb(m.id);
					} catch (e) {
						const status = e instanceof ApiError ? e.status : null;
						if (status === 503) unavailable = true;
						else if (status !== 409) startFailures[m.id] = 'Could not start the download.';
						// A download that runs already (409) is as good as one started here.
						if (status !== 409) return;
					}
					started[m.id] = { before };
					checked[m.id] = false;
				})
		);
		busy = false;
	}
</script>

<p>
	A ready-made list holds every title of a website, so Discover can search it. Discover needs one
	per website; getting it here saves fetching each one there.
</p>

{#if loadFailed}
	<p class="bad" role="alert">Could not load the websites.</p>
	<button class="btn sm" type="button" onclick={load}>Try again</button>
{:else if websites}
	<ul class="websites">
		{#each websites as module (module.id)}
			{@const event = currentEvent(module)}
			<li class="website" role="group" aria-label={module.name}>
				<label class="row">
					<input type="checkbox" disabled={running(module)} bind:checked={checked[module.id]} />
					<span class="grow">{module.name}</span>
				</label>
				{#if running(module)}
					<div class="job" role="status">
						<span class="small">Getting the ready-made list</span>
						{#if event && event.total > 0}
							<div
								class="bar"
								role="progressbar"
								aria-label="Progress of {module.name}"
								aria-valuemin={0}
								aria-valuemax={event.total}
								aria-valuenow={event.done}
							>
								<i style:width="{Math.round((event.done / event.total) * 100)}%"></i>
							</div>
						{/if}
					</div>
				{:else if event?.kind === 'finished'}
					<p class="small ok" role="status">Imported {event.titles ?? 0} titles.</p>
				{:else if event?.kind === 'failed'}
					<div class="failed small" role="alert">
						<p class="bad">{listFailedText(module, event.reason, event.job)}</p>
						{#if event.reason === 'no_dump'}
							<p class="muted">
								It stays without a list when you go on; Update list in Discover builds one.
							</p>
						{/if}
					</div>
				{:else if event?.kind === 'cancelled'}
					<p class="small muted" role="status">Cancelled.</p>
				{:else if module.list_size}
					<span class="small muted">
						<span class="num">{module.list_size.toLocaleString('en')}</span> titles · {listUpdatedText(
							module
						)}
					</span>
				{:else}
					<span class="small muted">No list yet.</span>
				{/if}
				{#if startFailures[module.id]}
					<p class="small bad" role="alert">{startFailures[module.id]}</p>
				{/if}
			</li>
		{/each}
	</ul>
	{#if unavailable}
		<p class="small bad" role="alert">
			This server can’t get ready-made lists now. Discover can get them later.
		</p>
	{:else}
		<button
			class="btn primary"
			type="button"
			disabled={busy || !websites.some((m) => checked[m.id])}
			onclick={download}>Download selected</button
		>
	{/if}
{:else}
	<p class="small muted">Loading the websites…</p>
{/if}

<p class="small muted">You can get or update lists later in Discover.</p>

<style>
	p {
		margin: 0;
	}
	.websites {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
	}
	.website,
	.job,
	.failed {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
	}
	.row {
		display: flex;
		gap: var(--sp-2);
		align-items: center;
	}
	.grow {
		flex: 1;
		min-width: 0;
	}
	.ok {
		color: var(--ok);
	}
	.bad {
		color: var(--bad);
	}
	.btn {
		align-self: flex-start;
	}
</style>
