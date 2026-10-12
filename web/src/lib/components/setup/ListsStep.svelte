<script lang="ts">
	import { ApiError } from '#lib/api/client.ts';
	import type { ListEvent, ModuleSummary } from '#lib/api/types.ts';
	import { listFailedText } from '#lib/discover/list-jobs.ts';
	import type { StepProps } from '#lib/setup/steps.ts';

	let { api, store, settings }: StepProps = $props();

	/** The websites selected in the step before, in the order the server lists them. */
	let websites = $state<ModuleSummary[] | null>(null);
	let failed = $state(false);
	/** The websites to download a ready-made list for; at first, the ones without a list. */
	let checked = $state<Record<string, boolean>>({});

	function load() {
		failed = false;
		api
			.listModules()
			.then((modules) => {
				const selected = new Set(settings.general.selected_websites);
				// A module can be listed once per website it serves; it has one list.
				const unique = modules.filter(
					(m, i) => selected.has(m.id) && modules.findIndex((o) => o.id === m.id) === i
				);
				checked = Object.fromEntries(unique.map((m) => [m.id, m.list_size === 0]));
				websites = unique;
			})
			.catch(() => (failed = true));
	}
	$effect(load);

	/** The websites whose download started here, or was running already. */
	let started = $state<Record<string, boolean>>({});
	/** Why a website's download did not start. */
	let failures = $state<Record<string, string>>({});
	let busy = $state(false);
	/** Whether the server can't run list jobs (503), so only Discover's later attempt is left. */
	let unavailable = $state(false);

	/** Whether a list job of `module` runs, by its latest event as `ListActions` has it. */
	function running(module: ModuleSummary, event: ListEvent | undefined): boolean {
		return event
			? event.kind === 'started' || event.kind === 'progress'
			: module.list_job_running || started[module.id] === true;
	}

	export function nextLabel(): string | undefined {
		return Object.values(started).some(Boolean) ? undefined : 'Skip';
	}

	/** Starts the ready-made list download of every checked website; each runs on its own. */
	async function download() {
		busy = true;
		await Promise.all(
			(websites ?? [])
				.filter((m) => checked[m.id])
				.map(async (m) => {
					delete failures[m.id];
					try {
						await api.importListDb(m.id);
						started[m.id] = true;
						checked[m.id] = false;
					} catch (e) {
						// A download that runs already is as good as one started here.
						if (e instanceof ApiError && e.status === 409) {
							started[m.id] = true;
							checked[m.id] = false;
						} else if (e instanceof ApiError && e.status === 503) unavailable = true;
						else failures[m.id] = 'Could not start the download.';
					}
				})
		);
		busy = false;
	}

	const updatedText = (module: ModuleSummary) =>
		module.list_updated
			? `updated ${new Date(module.list_updated).toLocaleDateString()}`
			: 'never updated';
</script>

<p>
	A ready-made list holds every title of a website, so Discover can search it. Discover needs one
	per website; getting it here saves fetching each one there.
</p>

{#if failed}
	<p class="bad" role="alert">Could not load the websites.</p>
	<button class="btn sm" type="button" onclick={load}>Try again</button>
{:else if websites}
	<ul class="websites">
		{#each websites as module (module.id)}
			{@const event = store.lists[module.id]}
			<li class="website" role="group" aria-label={module.name}>
				<label class="row">
					<input
						type="checkbox"
						disabled={running(module, event)}
						bind:checked={checked[module.id]}
					/>
					<span class="grow">{module.name}</span>
				</label>
				{#if running(module, event)}
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
							<p class="muted">It stays without a list when you go on.</p>
						{/if}
					</div>
				{:else if event?.kind === 'cancelled'}
					<p class="small muted" role="status">Cancelled.</p>
				{:else if module.list_size}
					<span class="small muted">
						<span class="num">{module.list_size.toLocaleString('en')}</span> titles · {updatedText(
							module
						)}
					</span>
				{:else}
					<span class="small muted">No list yet.</span>
				{/if}
				{#if failures[module.id]}
					<p class="small bad" role="alert">{failures[module.id]}</p>
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
