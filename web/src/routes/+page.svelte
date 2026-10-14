<script lang="ts">
	import { api, events } from '#lib/app.ts';
	import { ApiError } from '#lib/api/client.ts';
	import type { FavoriteView, GroupView, SeriesStatus } from '#lib/api/types.ts';
	import {
		CHIPS,
		SORTS,
		chipCounts,
		emptyFilters,
		facets,
		filterFavorites,
		isFailing,
		websites,
		type LibraryFilters
	} from '#lib/library/filters.ts';
	import { sections, type Section } from '#lib/library/sections.ts';
	import { FORMAT, UNKNOWN } from '#lib/discover/filters.ts';
	import {
		inLibrary,
		selectShown,
		selectionLabel,
		toggle as toggleSelected
	} from '#lib/library/selection.ts';
	import { seriesHref } from '#lib/series/href.ts';
	import CoverThumb from '#lib/components/CoverThumb.svelte';
	import GenreChips from '#lib/components/discover/GenreChips.svelte';
	import ImportDialog from '#lib/components/library/ImportDialog.svelte';
	import GroupsDialog from '#lib/components/library/GroupsDialog.svelte';
	import BulkGroupDialog from '#lib/components/library/BulkGroupDialog.svelte';
	import BulkFolderDialog from '#lib/components/library/BulkFolderDialog.svelte';
	import type { BulkChange } from '#lib/api/types.ts';

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
	/** Whether cards select series instead of opening them. */
	let selecting = $state(false);
	/** Favorite ids; kept while filters hide them. */
	let selected = $state<Set<number>>(new Set());
	let bulkBusy = $state(false);
	let bulkError = $state<string | null>(null);
	let bulkNote = $state<string | null>(null);
	/** The bulk action waiting in a dialog or for confirmation. */
	let bulkAction = $state<'group' | 'folder' | 'remove' | null>(null);

	const shown = $derived(filterFavorites(favorites, filters));
	const counts = $derived(chipCounts(favorites));
	const sites = $derived(websites(favorites));
	const libraryFacets = $derived(facets(favorites));
	/** Whether a tag is included or excluded; its chips stay while it is, even with no tags left. */
	const tagFilter = $derived(Object.values(filters.tags).some((s) => s !== 'ignore'));
	/** The formats in the library, plus a chosen one it no longer holds. */
	const formats = $derived(
		filters.format && !libraryFacets.formats.some((f) => f.value === filters.format)
			? [...libraryFacets.formats, { value: filters.format, count: 0 }]
			: libraryFacets.formats
	);
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
	/** The cards on screen: those in folded sections are hidden too. */
	const visible = $derived(
		shownSections ? shownSections.filter((s) => !s.collapsed).flatMap((s) => s.favorites) : shown
	);
	const selectedLabel = $derived(selectionLabel(selected, visible));
	/** Paused series in the selection, which "Check now" skips. */
	const selectedPaused = $derived(favorites.filter((f) => selected.has(f.id) && !f.enabled).length);
	const job = $derived(events.jobs[JOB]);
	const checking = $derived(starting || job?.state === 'running');

	/** `unknown` is the series without a MangaBaka match; a format `FORMAT` lacks shows as is. */
	function formatLabel(format: string): string {
		return format === UNKNOWN ? 'Unknown' : (FORMAT[format] ?? format);
	}

	function readUngroupedCollapsed(): boolean {
		try {
			return localStorage.getItem(UNGROUPED_KEY) === 'true';
		} catch {
			return false;
		}
	}

	function load(): Promise<void> {
		// Without its groups, the library still shows, as one grid.
		return Promise.all([api.listFavorites(), api.listGroups().catch(() => [])])
			.then(([list, groupList]) => {
				favorites = list;
				groups = groupList;
				error = null;
				// Removed series leave the selection.
				selected = inLibrary(selected, list);
			})
			.catch(() => {
				error = 'Could not load the library.';
			})
			.finally(() => (loading = false));
	}
	$effect(() => {
		load();
	});

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

	/** Why a check did not start. */
	function checkFailure(e: unknown): string {
		return e instanceof ApiError && e.status === 409
			? 'A check is already running.'
			: e instanceof ApiError && e.status === 503
				? 'The new-chapter check is not running on this server.'
				: 'Could not start the check.';
	}

	async function checkNow() {
		starting = true;
		checkError = null;
		try {
			await api.checkFavorites();
		} catch (e) {
			checkError = checkFailure(e);
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

	function startSelecting() {
		selecting = true;
		bulkError = null;
		bulkNote = null;
	}

	/** Leaves selection mode, clearing the selection. */
	function stopSelecting() {
		selecting = false;
		selected = new Set();
		bulkAction = null;
		bulkError = null;
		bulkNote = null;
	}

	function onkeydown(event: KeyboardEvent) {
		// An open dialog closes on its own Esc; the bar's confirmation cancels first.
		if (event.key !== 'Escape' || !selecting || event.defaultPrevented) return;
		if (document.querySelector('dialog[open]')) return;
		if (bulkAction === 'remove') bulkAction = null;
		else stopSelecting();
	}

	/** Applies `change` to the selection, then reloads the library; rejects when it fails. */
	async function applyBulk(change: Omit<BulkChange, 'ids'>) {
		bulkBusy = true;
		bulkError = null;
		bulkNote = null;
		try {
			await api.bulkUpdateFavorites({ ids: [...selected], ...change });
		} finally {
			bulkBusy = false;
		}
		await load();
	}

	/** Runs an action from the bar, saying so when it fails. */
	async function bulk(change: Omit<BulkChange, 'ids'>, failed: string) {
		try {
			await applyBulk(change);
		} catch {
			bulkError = failed;
		} finally {
			// The bar shows the outcome in place of the confirmation.
			if (change.remove) bulkAction = null;
		}
	}

	async function checkSelected() {
		bulkBusy = true;
		bulkError = null;
		bulkNote = null;
		try {
			await api.checkFavorites([...selected]);
			if (selectedPaused > 0) {
				bulkNote = `Checking; the ${selectedPaused} paused series ${selectedPaused === 1 ? 'is' : 'are'} skipped.`;
			}
		} catch (e) {
			bulkError = checkFailure(e);
		} finally {
			bulkBusy = false;
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
				{#if selecting}
					<button
						class="card selectable"
						class:disabled={!favorite.enabled}
						class:selected={selected.has(favorite.id)}
						type="button"
						role="checkbox"
						aria-checked={selected.has(favorite.id)}
						aria-label={favorite.display_title}
						onclick={() => (selected = toggleSelected(selected, favorite.id))}
					>
						{@render card(favorite)}
						<span class="tick" aria-hidden="true">{selected.has(favorite.id) ? '✓' : ''}</span>
					</button>
				{:else}
					<a class="card" class:disabled={!favorite.enabled} href={seriesHref(favorite)}>
						{@render card(favorite)}
					</a>
				{/if}
			</li>
		{/each}
	</ul>
{/snippet}

{#snippet card(favorite: FavoriteView)}
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
		{#if isFailing(favorite)}
			<span class="badge failing" title="The website hasn't answered its last checks"
				>Not found</span
			>
		{/if}
		<!-- 0 until its first check: no count to show. -->
		{#if favorite.current_chapter > 0}
			<span
				class="badge count num"
				aria-label="{favorite.current_chapter} chapters"
				title="{favorite.current_chapter} chapters">{favorite.current_chapter}</span
			>
		{/if}
	</span>
	<!-- The website's own title, when MangaBaka's shows instead. -->
	<span class="t" title={favorite.display_title === favorite.title ? undefined : favorite.title}
		>{favorite.display_title}</span
	>
	<span class="small muted"
		>{favorite.website} · {favorite.enabled ? STATUS[favorite.status] : 'Paused'}</span
	>
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
			{#if selecting}
				<button
					class="btn"
					type="button"
					onclick={() => (selected = selectShown(selected, visible))}>Select all</button
				>
				<button class="btn" type="button" onclick={stopSelecting}>Done</button>
			{:else}
				<button class="btn" type="button" onclick={startSelecting}>Select</button>
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
	{#if libraryFacets.tags.length > 0 || tagFilter || formats.length > 1}
		<div class="facets">
			{#if formats.length > 1}
				<label class="field">
					<span class="label">Format</span>
					<select class="input" bind:value={filters.format}>
						<option value="">Any</option>
						{#each formats as format (format.value)}
							<option value={format.value}>{formatLabel(format.value)} ({format.count})</option>
						{/each}
					</select>
				</label>
			{/if}
			{#if libraryFacets.tags.length > 0 || tagFilter}
				<GenreChips genres={libraryFacets.tags} bind:states={filters.tags} label="Tags" />
			{/if}
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
					{#if selecting && !section.collapsed}
						<button
							class="btn sm ghost"
							type="button"
							aria-label="Select all in {section.name}"
							onclick={() => (selected = selectShown(selected, section.favorites))}
							>Select all</button
						>
					{/if}
				</h2>
				<div {id} hidden={section.collapsed}>
					{@render grid(section.favorites, section.name)}
				</div>
			</section>
		{/each}
	{:else}
		{@render grid(shown, 'Favorites')}
	{/if}

	{#if selecting && selected.size > 0}
		<div class="bulk-bar" role="region" aria-label="Selected series">
			{#if bulkAction === 'remove'}
				<span class="grow small" role="alert"
					>Remove {selected.size} series from the library? Downloaded chapters stay recorded.</span
				>
				<button
					class="btn sm primary"
					type="button"
					disabled={bulkBusy}
					onclick={() => bulk({ remove: true }, 'Could not remove the series.')}>Remove</button
				>
				<button class="btn sm ghost" type="button" onclick={() => (bulkAction = null)}
					>Cancel</button
				>
			{:else}
				<span class="grow small num" role="status" aria-label="Selection">
					{selectedLabel}
					{#if bulkError}
						· <span class="bad">{bulkError}</span>
					{:else if bulkNote}
						· {bulkNote}
					{:else if selectedPaused > 0}
						· <span class="muted">Check now skips the paused ones.</span>
					{/if}
				</span>
				<div class="bulk-actions">
					<button
						class="btn sm"
						type="button"
						disabled={bulkBusy}
						onclick={() => bulk({ enabled: false }, 'Could not pause the series.')}>Pause</button
					>
					<button
						class="btn sm"
						type="button"
						disabled={bulkBusy}
						onclick={() => bulk({ enabled: true }, 'Could not resume the series.')}>Resume</button
					>
					<button
						class="btn sm"
						type="button"
						disabled={bulkBusy}
						onclick={() => (bulkAction = 'group')}>Group…</button
					>
					<button
						class="btn sm"
						type="button"
						disabled={bulkBusy}
						onclick={() => (bulkAction = 'folder')}>Download folder…</button
					>
					<button class="btn sm" type="button" disabled={bulkBusy} onclick={checkSelected}
						>Check now</button
					>
					<button
						class="btn sm"
						type="button"
						disabled={bulkBusy}
						title="Marks the chapters found by their last check seen, without downloading them"
						onclick={() => bulk({ mark_seen: true }, 'Could not mark the series seen.')}
						>Mark all seen</button
					>
					<button
						class="btn sm"
						type="button"
						disabled={bulkBusy}
						onclick={() => (bulkAction = 'remove')}>Remove…</button
					>
				</div>
			{/if}
		</div>
	{/if}
	{#if bulkAction === 'group'}
		<BulkGroupDialog
			{api}
			count={selected.size}
			onpick={(group_id) => applyBulk({ group_id })}
			onclose={() => (bulkAction = null)}
		/>
	{:else if bulkAction === 'folder'}
		<BulkFolderDialog
			{api}
			count={selected.size}
			onpick={(destination) => applyBulk({ destination })}
			onclose={() => (bulkAction = null)}
		/>
	{/if}
</div>

<svelte:window {onkeydown} />

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
	.sort,
	.field {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
	}
	.facets {
		display: flex;
		flex-direction: column;
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
	.card.selectable {
		position: relative;
		width: 100%;
		border: 2px solid transparent;
		border-radius: var(--r-lg);
		background: none;
		padding: 4px;
		font: inherit;
		text-align: start;
		cursor: pointer;
	}
	.card.selected {
		border-color: var(--accent);
		background: var(--surface-2);
	}
	.tick {
		position: absolute;
		top: 10px;
		left: 10px;
		width: 22px;
		height: 22px;
		display: grid;
		place-items: center;
		border: 2px solid var(--accent);
		border-radius: var(--r-pill);
		background: var(--surface);
		color: var(--accent-fg);
		font-weight: 700;
		box-shadow: var(--shadow);
	}
	.card.selected .tick {
		background: var(--accent);
	}
	.section h2 {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
	}
	/* Where the queue dock sits, as the Settings page's save bar. */
	.bulk-bar {
		position: fixed;
		left: var(--sp-2);
		right: var(--sp-2);
		bottom: calc(var(--bottom-nav-h) + var(--safe-bottom) + var(--sp-2));
		z-index: 30;
		max-width: 900px;
		margin: 0 auto;
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: var(--sp-2);
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-xl);
		box-shadow: var(--shadow);
		padding: var(--sp-2) var(--sp-3);
	}
	.bulk-actions {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-2);
	}
	.grow {
		flex: 1 1 200px;
		min-width: 0;
	}
	@media (min-width: 861px) {
		.bulk-bar {
			left: var(--sp-4);
			right: var(--sp-4);
			bottom: calc(var(--sp-4) + var(--safe-bottom));
			padding: 10px 14px;
		}
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
	.badge.failing {
		right: auto;
		left: 6px;
		background: var(--bad-soft);
		color: var(--bad);
	}
	/* Quieter than the accent "+N", which stays the eye-catcher. */
	.badge.count {
		top: auto;
		bottom: 6px;
		background: var(--surface-2);
		color: var(--muted);
		box-shadow: none;
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
