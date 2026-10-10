<script lang="ts">
	import { api, events } from '#lib/app.ts';
	import { ApiError } from '#lib/api/client.ts';
	import type { FavoriteView, SeriesStatus } from '#lib/api/types.ts';
	import {
		CHIPS,
		SORTS,
		chipCounts,
		emptyFilters,
		filterFavorites,
		websites,
		type LibraryFilters
	} from '#lib/library/filters.ts';
	import { seriesHref } from '#lib/series/href.ts';
	import FolderDialog from '#lib/components/library/FolderDialog.svelte';
	import ImportDialog from '#lib/components/library/ImportDialog.svelte';

	/** The background job behind "Check now" (`GET /api/jobs/favorites`). */
	const JOB = 'favorites';

	const STATUS: Record<SeriesStatus, string> = {
		ongoing: 'Ongoing',
		completed: 'Completed',
		hiatus: 'Hiatus',
		cancelled: 'Cancelled',
		unknown: 'Status unknown'
	};

	let favorites = $state<FavoriteView[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let filters = $state<LibraryFilters>(emptyFilters());
	/** Covers that failed to load, by favorite id. */
	let brokenCovers = $state<Record<number, boolean>>({});
	let starting = $state(false);
	let checkError = $state<string | null>(null);
	let importing = $state(false);
	/** The series whose download folder the dialog changes. */
	let folderOf = $state<FavoriteView | null>(null);

	const shown = $derived(filterFavorites(favorites, filters));
	const counts = $derived(chipCounts(favorites));
	const sites = $derived(websites(favorites));
	const job = $derived(events.jobs[JOB]);
	const checking = $derived(starting || job?.state === 'running');

	function load() {
		api
			.listFavorites()
			.then((list) => {
				favorites = list;
				error = null;
			})
			.catch(() => (error = 'Could not load the library.'))
			.finally(() => (loading = false));
	}
	$effect(load);

	// The job's state on arrival, so a check already going shows its progress.
	$effect(() => {
		api
			.listJobs()
			.then((list) => events.seed({ jobs: list }))
			.catch(() => {});
	});

	// A check that ended changed the favorites: reload them.
	let lastEnd: unknown = null;
	$effect(() => {
		const event = events.favorites;
		if (!event || event.kind === 'started' || event.kind === 'progress' || event === lastEnd) {
			return;
		}
		lastEnd = event;
		load();
	});

	async function checkNow() {
		starting = true;
		checkError = null;
		try {
			await api.checkFavorites();
		} catch (e) {
			checkError =
				e instanceof ApiError && e.status === 409
					? 'A check is already running.'
					: e instanceof ApiError && e.status === 503
						? 'The new-chapter check is not running on this server.'
						: 'Could not start the check.';
		} finally {
			starting = false;
		}
	}

	async function cancel() {
		try {
			await api.cancelJob(JOB);
		} catch {
			checkError = 'Could not cancel the check.';
		}
	}

	/** A stable hue per title for its placeholder cover. */
	function hue(title: string): number {
		let h = 0;
		for (const c of title) h = (h * 31 + c.charCodeAt(0)) % 360;
		return h;
	}
</script>

<svelte:head><title>Library · FMD2r</title></svelte:head>

<div class="page">
	<div class="head">
		<h1>Library</h1>
		<div class="check">
			{#if checking}
				<div
					class="bar"
					role="progressbar"
					aria-label="Check progress"
					aria-valuemin={0}
					aria-valuemax={job?.total || undefined}
					aria-valuenow={job?.total ? job.done : undefined}
				>
					<i style:width="{job?.total ? Math.round((job.done / job.total) * 100) : 30}%"></i>
				</div>
				<span class="small muted num" role="status">
					Checking{job?.total ? ` ${job.done}/${job.total}` : '…'}
				</span>
				<button class="btn sm ghost" type="button" onclick={cancel}>Cancel</button>
			{:else}
				<button class="btn primary" type="button" onclick={checkNow}>Check now</button>
			{/if}
			<button class="btn" type="button" onclick={() => (importing = true)}>Import from FMD2</button>
		</div>
	</div>
	{#if checkError}
		<p class="bad" role="alert">{checkError}</p>
	{/if}
	{#if folderOf}
		<FolderDialog
			{api}
			favorite={folderOf}
			onsaved={(updated) => {
				favorites = favorites.map((f) => (f.id === updated.id ? updated : f));
			}}
			onclose={() => (folderOf = null)}
		/>
	{/if}
	{#if importing}
		<ImportDialog
			{api}
			job={events.jobs['import']}
			onimported={load}
			onclose={() => (importing = false)}
		/>
	{/if}

	<div class="toolbar">
		<input
			class="input search"
			type="search"
			placeholder="Search the library"
			aria-label="Search the library"
			bind:value={filters.q}
		/>
		<label class="sort">
			<span class="label">Sort</span>
			<select class="input" bind:value={filters.sort}>
				{#each SORTS as sort (sort.id)}
					<option value={sort.id}>{sort.label}</option>
				{/each}
			</select>
		</label>
	</div>

	<div class="chips" role="group" aria-label="Show">
		{#each CHIPS as chip (chip.id)}
			<button
				class="chip"
				type="button"
				aria-pressed={filters.chip === chip.id}
				onclick={() => (filters.chip = chip.id)}
				>{chip.label} <span class="count num">{counts[chip.id]}</span></button
			>
		{/each}
	</div>
	{#if sites.length > 1}
		<div class="chips" role="group" aria-label="Website">
			<button
				class="chip"
				type="button"
				aria-pressed={filters.website === ''}
				onclick={() => (filters.website = '')}>Every website</button
			>
			{#each sites as site (site.id)}
				<button
					class="chip"
					type="button"
					aria-pressed={filters.website === site.id}
					onclick={() => (filters.website = site.id)}
					>{site.name} <span class="count num">{site.count}</span></button
				>
			{/each}
		</div>
	{/if}

	{#if error}
		<p class="bad" role="alert">{error}</p>
	{:else if loading}
		<p class="muted">Loading the library…</p>
	{:else if favorites.length === 0}
		<p class="muted empty">
			The library is empty. Open a series (paste its URL above, or find it in Discover) and add it
			to the library to have new chapters checked.
		</p>
	{:else if shown.length === 0}
		<p class="muted empty">No favorites match.</p>
	{:else}
		<ul class="grid" aria-label="Favorites">
			{#each shown as favorite (favorite.id)}
				<li>
					<a class="card" class:disabled={!favorite.enabled} href={seriesHref(favorite)}>
						<span class="art">
							{#if favorite.cover_url && !brokenCovers[favorite.id]}
								<img
									class="cover"
									src={favorite.cover_url}
									alt=""
									loading="lazy"
									onerror={() => (brokenCovers[favorite.id] = true)}
								/>
							{:else}
								<span class="cover blank" style:--h={hue(favorite.title)} aria-hidden="true"
									>{favorite.title}</span
								>
							{/if}
							{#if favorite.new_chapters > 0}
								<span class="badge num" aria-label="{favorite.new_chapters} new chapters"
									>+{favorite.new_chapters}</span
								>
							{/if}
						</span>
						<span class="t">{favorite.title}</span>
						<span class="small muted"
							>{favorite.website} · {favorite.enabled
								? STATUS[favorite.status]
								: 'Not checked'}</span
						>
					</a>
					<button
						class="btn ghost sm folder"
						type="button"
						aria-label="Download folder of {favorite.title}"
						title={favorite.save_to}
						onclick={() => (folderOf = favorite)}>Folder…</button
					>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--sp-3);
		flex-wrap: wrap;
	}
	.check {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
	}
	.bar {
		width: 120px;
		height: 6px;
		border-radius: var(--r-pill);
		background: var(--surface-2);
		overflow: hidden;
	}
	.bar i {
		display: block;
		height: 100%;
		background: var(--accent);
		transition: width 0.3s;
	}
	.toolbar {
		display: flex;
		gap: var(--sp-3);
		align-items: center;
		flex-wrap: wrap;
	}
	.search {
		flex: 1 1 240px;
		font-size: var(--fs-search);
		padding: 8px 12px;
	}
	.sort {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
	}
	.chips {
		display: flex;
		gap: 6px;
		flex-wrap: wrap;
	}
	.chip {
		border: 1px solid var(--line);
		background: var(--surface);
		color: var(--fg);
		border-radius: var(--r-pill);
		padding: 4px 12px;
		font: inherit;
		font-size: var(--fs-sm);
		cursor: pointer;
	}
	.chip[aria-pressed='true'] {
		background: var(--accent);
		border-color: var(--accent);
		color: var(--accent-fg);
	}
	.count {
		opacity: 0.7;
	}
	.grid {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
		gap: 20px 16px;
	}
	.folder {
		margin-top: 2px;
	}
	.card {
		display: flex;
		flex-direction: column;
		gap: 6px;
		color: inherit;
		text-decoration: none;
	}
	.card.disabled .art {
		opacity: 0.5;
	}
	.card:hover .cover {
		outline: 2px solid var(--accent);
		outline-offset: 2px;
	}
	.art {
		position: relative;
		display: block;
	}
	.cover {
		display: block;
		width: 100%;
		aspect-ratio: 5 / 7;
		border-radius: 4px;
		object-fit: cover;
		background: var(--surface-2);
	}
	.cover.blank {
		display: flex;
		align-items: flex-end;
		padding: 6px;
		color: var(--on-cover);
		font: 700 var(--fs-ui)/1.1 var(--f-display);
		background: linear-gradient(
			160deg,
			hsl(var(--h) var(--cover-tone-top)),
			hsl(calc(var(--h) + 40) var(--cover-tone-bottom))
		);
		text-shadow: var(--on-cover-shadow);
		overflow: hidden;
	}
	.badge {
		position: absolute;
		top: 6px;
		right: 6px;
		border-radius: var(--r-pill);
		padding: 1px 8px;
		font-size: var(--fs-sm);
		font-weight: 700;
		background: var(--accent);
		color: var(--accent-fg);
		box-shadow: var(--shadow);
	}
	.t {
		font-weight: 600;
		font-size: var(--fs-card);
		line-height: 1.25;
	}
	.empty,
	p {
		margin: 0;
	}
	.bad {
		color: var(--bad);
	}
	@media (max-width: 860px) {
		.grid {
			grid-template-columns: repeat(auto-fill, minmax(110px, 1fr));
		}
	}
</style>
