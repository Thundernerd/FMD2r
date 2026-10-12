<script lang="ts">
	import { api, events } from '#lib/app.ts';
	import { ApiError } from '#lib/api/client.ts';
	import type { FavoriteView, GroupView, SeriesStatus } from '#lib/api/types.ts';
	import {
		CHIPS,
		SORTS,
		chipCounts,
		emptyFilters,
		filterFavorites,
		websites,
		type LibraryFilters
	} from '#lib/library/filters.ts';
	import { sections, type Section } from '#lib/library/sections.ts';
	import { seriesHref } from '#lib/series/href.ts';
	import CoverThumb from '#lib/components/CoverThumb.svelte';
	import ImportDialog from '#lib/components/library/ImportDialog.svelte';
	import GroupsDialog from '#lib/components/library/GroupsDialog.svelte';

	/** Where this browser keeps whether "Ungrouped" is folded; groups keep theirs on the server. */
	const UNGROUPED_KEY = 'fmd2r.library.ungrouped-collapsed';

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
	let starting = $state(false);
	let checkError = $state<string | null>(null);
	let importing = $state(false);
	let groups = $state<GroupView[]>([]);
	let editingGroups = $state(false);
	let foldError = $state<string | null>(null);
	let ungroupedCollapsed = $state(readUngroupedCollapsed());

	const shown = $derived(filterFavorites(favorites, filters));
	const counts = $derived(chipCounts(favorites));
	const sites = $derived(websites(favorites));
	/** With no groups, the grid is not split. */
	const shownSections = $derived(
		groups.length > 0
			? sections(favorites, groups, filters).map((s) =>
					s.group ? s : { ...s, collapsed: ungroupedCollapsed }
				)
			: null
	);
	const groupCounts = $derived(
		favorites.reduce<Record<number, number>>((counts, f) => {
			if (f.group_id != null) counts[f.group_id] = (counts[f.group_id] ?? 0) + 1;
			return counts;
		}, {})
	);
	const job = $derived(events.jobs[JOB]);
	const checking = $derived(starting || job?.state === 'running');

	function readUngroupedCollapsed(): boolean {
		try {
			return localStorage.getItem(UNGROUPED_KEY) === 'true';
		} catch {
			return false;
		}
	}

	function load() {
		// Without its groups, the library still shows, as one grid.
		Promise.all([api.listFavorites(), api.listGroups().catch(() => [])])
			.then(([list, groupList]) => {
				favorites = list;
				groups = groupList;
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

	/** Folds or unfolds a section; a group's fold state is saved for every device. */
	async function toggle(section: Section) {
		const collapsed = !section.collapsed;
		if (!section.group) {
			ungroupedCollapsed = collapsed;
			try {
				localStorage.setItem(UNGROUPED_KEY, String(collapsed));
			} catch {
				// Folded for this visit only.
			}
			return;
		}
		const { id } = section.group;
		foldError = null;
		try {
			// Folded once saved, so the section shows what every device will see.
			const saved = await api.updateGroup(id, { collapsed });
			groups = groups.map((g) => (g.id === id ? saved : g));
		} catch {
			foldError = `Could not save whether ${section.name} is folded.`;
		}
	}

	async function cancel() {
		try {
			await api.cancelJob(JOB);
		} catch {
			checkError = 'Could not cancel the check.';
		}
	}
</script>

<svelte:head><title>Library · FMD2r</title></svelte:head>

{#snippet grid(list: FavoriteView[], label: string)}
	<ul class="grid" aria-label={label}>
		{#each list as favorite (favorite.id)}
			<li>
				<a class="card" class:disabled={!favorite.enabled} href={seriesHref(favorite)}>
					<span class="art">
						<!-- A check can replace the series cover with the website's: start that one afresh. -->
						{#key favorite.cover_url}
							<CoverThumb src={favorite.cover_url} title={favorite.display_title} />
						{/key}
						{#if favorite.new_chapters > 0}
							<span class="badge num" aria-label="{favorite.new_chapters} new chapters"
								>+{favorite.new_chapters}</span
							>
						{/if}
					</span>
					<!-- The website's own title, when MangaBaka's shows instead. -->
					<span
						class="t"
						title={favorite.display_title === favorite.title ? undefined : favorite.title}
						>{favorite.display_title}</span
					>
					<span class="small muted"
						>{favorite.website} · {favorite.enabled ? STATUS[favorite.status] : 'Not checked'}</span
					>
				</a>
			</li>
		{/each}
	</ul>
{/snippet}

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
			<button class="btn" type="button" onclick={() => (editingGroups = true)}>Groups…</button>
			<button class="btn" type="button" onclick={() => (importing = true)}>Import from FMD2</button>
		</div>
	</div>
	{#if checkError}
		<p class="bad" role="alert">{checkError}</p>
	{/if}
	{#if importing}
		<ImportDialog
			{api}
			job={events.jobs['import']}
			onimported={load}
			onclose={() => (importing = false)}
		/>
	{/if}
	{#if foldError}
		<p class="bad" role="alert">{foldError}</p>
	{/if}
	{#if editingGroups}
		<GroupsDialog
			{api}
			counts={groupCounts}
			onchange={(list) => {
				groups = list;
				// A deleted group's series are ungrouped now.
				load();
			}}
			onclose={() => (editingGroups = false)}
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
	{:else if shownSections}
		{#each shownSections as section (section.group?.id ?? 'ungrouped')}
			{@const id = `section-${section.group?.id ?? 'ungrouped'}`}
			<section class="section">
				<h2>
					<button
						class="section-head"
						type="button"
						aria-expanded={!section.collapsed}
						aria-controls={id}
						onclick={() => toggle(section)}
					>
						<span class="chevron" aria-hidden="true">{section.collapsed ? '▸' : '▾'}</span>
						{section.name} · <span class="num">{section.favorites.length}</span>
					</button>
				</h2>
				<div {id} hidden={section.collapsed}>
					{@render grid(section.favorites, section.name)}
				</div>
			</section>
		{/each}
	{:else}
		{@render grid(shown, 'Favorites')}
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
		flex-wrap: wrap;
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
	.section {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
	}
	.section h2 {
		margin: 0;
		font-size: var(--fs-lg);
	}
	.section-head {
		display: inline-flex;
		align-items: center;
		gap: var(--sp-2);
		border: 0;
		background: none;
		color: inherit;
		font: inherit;
		font-weight: 700;
		padding: 2px 0;
		cursor: pointer;
	}
	.chevron {
		width: 1em;
		opacity: 0.7;
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
	.art {
		position: relative;
		display: block;
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
