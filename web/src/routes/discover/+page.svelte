<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { afterNavigate, goto, snapshot } from '$app/navigation';
	import { page as route } from '$app/state';
	import type { ListFacets, ListItem, MangaBakaStatus, ModuleSummary } from '#lib/api/types.ts';
	import { api, events } from '#lib/app.ts';
	import CoverThumb from '#lib/components/discover/CoverThumb.svelte';
	import GenreChips from '#lib/components/discover/GenreChips.svelte';
	import ListActions from '#lib/components/discover/ListActions.svelte';
	import MangaBakaHint from '#lib/components/discover/MangaBakaHint.svelte';
	import WebsitePicker from '#lib/components/discover/WebsitePicker.svelte';
	import { seriesHref } from '#lib/series/href.ts';
	import {
		emptyFilters,
		facetQuery,
		filtersFromQuery,
		FORMAT,
		PUBLICATION,
		queryString,
		searchQuery,
		STATUS,
		UNKNOWN,
		type Filters,
		type Tri
	} from '#lib/discover/filters.ts';

	/** How long typing pauses before the search runs. */
	const DEBOUNCE_MS = 250;
	const NO_FACETS: ListFacets = { genres: [], statuses: [], formats: [], publications: [] };

	let modules = $state<ModuleSummary[] | null>(null);
	/** `general.selected_websites`, once loaded. */
	let websites = $state<string[] | null>(null);
	/** `general.load_covers`: off shows only placeholders. Off until the settings load. */
	let loadCovers = $state(false);
	// The filters start from the URL, so a reload, a shared link or Back opens the same search.
	const initial = filtersFromQuery(route.url.searchParams);
	let module = $state(initial.module);
	let text = $state(initial.q);
	let q = $state(initial.q);
	let genres = $state<Record<string, Tri>>(initial.genres);
	let status = $state(initial.status);
	let format = $state(initial.format);
	let publication = $state(initial.publication);
	let mangabaka = $state<MangaBakaStatus | null>(null);
	let filtersOpen = $state(false);

	let items = $state<ListItem[]>([]);
	let total = $state(0);
	let page = $state(1);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let facets = $state<ListFacets>(NO_FACETS);

	const selected = $derived(modules?.find((m) => m.id === module));
	const names = $derived(new Map(modules?.map((m) => [m.id, m.name])));
	/** Whether no loaded website is selected, so there is nothing to list. */
	const noWebsites = $derived(
		modules !== null && websites !== null && !modules.some((m) => websites?.includes(m.id))
	);
	const filters = $derived<Filters>({ module, q, genres, status, format, publication, page: 1 });
	/** The filters as Discover's query string. */
	const query = $derived(queryString(filters));
	const more = $derived(items.length < total);

	function loadModules() {
		api
			.listModules()
			.then((list) => (modules = list))
			.catch(() => (error = 'Could not load the websites.'));
	}
	$effect(loadModules);
	$effect(() => {
		api
			.getSettings()
			.then((settings) => {
				websites = settings.general.selected_websites;
				loadCovers = settings.general.load_covers;
			})
			.catch(() => (error = 'Could not load the selected websites.'));
	});

	$effect(() => {
		api
			.mangabakaStatus()
			.then((s) => (mangabaka = s))
			.catch(() => (mangabaka = null));
	});

	$effect(() => {
		const value = text;
		const timer = setTimeout(() => (q = value), DEBOUNCE_MS);
		return () => clearTimeout(timer);
	});

	/** Shows the filters in the URL, replacing its history entry so Back leaves Discover. */
	$effect(() => {
		const next = query;
		// Rewritten also when it differs only in dropped or reordered values.
		const shown = untrack(() => route.url.search.replace(/^\?/, ''));
		if (next !== shown) {
			void goto(next ? `/discover?${next}` : '/discover', {
				replaceState: true,
				reset: false
			});
		}
	});

	function apply(next: Filters) {
		module = next.module;
		text = q = next.q;
		genres = next.genres;
		status = next.status;
		format = next.format;
		publication = next.publication;
	}

	/** How Discover was last arrived at; only Back and Forward restore a snapshot. */
	let arrival: string | null = null;

	afterNavigate((navigation) => {
		arrival = navigation.type;
		// A link to other filters while on Discover (such as the nav bar's) applies them.
		const url = navigation.to?.url;
		if (navigation.type === 'goto' || url?.pathname !== '/discover') return;
		const next = filtersFromQuery(url.searchParams);
		if (queryString(next) !== query) apply(next);
	});

	/** Bumped by every new search, so answers to an older one are dropped. */
	let generation = 0;

	interface DiscoverSnapshot {
		query: string;
		text: string;
		items: ListItem[];
		total: number;
		page: number;
		scrollY: number;
	}

	// Back to Discover shows the results and scroll position it left. SvelteKit restores
	// the scroll before the snapshot, while the grid is still empty, so the snapshot does it.
	// A reload also finds a snapshot, but starts afresh from page 1.
	snapshot<DiscoverSnapshot>({
		capture: () => ({ query, text, items: $state.snapshot(items), total, page, scrollY }),
		restore: (saved) => {
			if (arrival !== 'popstate') return;
			text = saved.text;
			if (saved.query !== query) return;
			// Drops the page-1 search that started on mounting.
			generation++;
			items = saved.items;
			total = saved.total;
			page = saved.page;
			loading = false;
			error = null;
			void tick().then(() => scrollTo(scrollX, saved.scrollY));
		}
	});

	async function load(next: number, reset: boolean) {
		const mine = reset ? ++generation : generation;
		loading = true;
		try {
			const result = await api.searchLists(searchQuery({ ...filters, page: next }));
			if (mine !== generation) return;
			items = reset ? result.items : [...items, ...result.items];
			total = result.total;
			page = next;
			error = null;
		} catch {
			if (mine === generation) error = 'Could not search the lists.';
		} finally {
			if (mine === generation) loading = false;
		}
	}

	/** Only the website and the text narrow the facets. */
	function loadFacets() {
		api
			.listFacets(facetQuery({ ...emptyFilters(), module, q }))
			.then((f) => (facets = f))
			.catch(() => (facets = NO_FACETS));
	}

	function reload() {
		void load(1, true);
		loadFacets();
	}

	$effect(() => {
		// A new search whenever a filter changes.
		void query;
		void load(1, true);
	});
	$effect(loadFacets);

	function loadMore() {
		if (more && !loading) void load(page + 1, false);
	}

	/** Loads the next page when `node` scrolls into view. */
	function infinite(node: HTMLElement) {
		const observer = new IntersectionObserver((entries) => {
			if (entries.some((e) => e.isIntersecting)) loadMore();
		});
		observer.observe(node);
		return { destroy: () => observer.disconnect() };
	}

	/** The website, format and status under a title: MangaBaka's status when it knows one,
	 * else the list's. */
	function subtitle(item: ListItem): string {
		const site = names.get(item.module_id) ?? item.module_id;
		const status = PUBLICATION[item.publication] ?? STATUS[item.status];
		return [site, FORMAT[item.format], status].filter(Boolean).join(' · ');
	}

	/** The options of a MangaBaka facet: every known value, then the titles without one. */
	function options(labels: Record<string, string>, counts: ListFacets['formats']) {
		const count = (value: string) => counts.find((c) => c.value === value)?.count ?? 0;
		return [
			...Object.entries(labels).map(([value, label]) => ({ value, label, count: count(value) })),
			{ value: UNKNOWN, label: 'Unknown', count: count(UNKNOWN) }
		];
	}

	function afterJob() {
		loadModules();
		reload();
	}
</script>

<svelte:head><title>Discover · FMD2r</title></svelte:head>

<div class="page">
	<h1>Discover</h1>
	<MangaBakaHint status={mangabaka} />

	{#if noWebsites}
		<p class="muted empty">
			No websites are selected. <a href="/settings#section-websites">Choose websites</a> to list and search
			here.
		</p>
	{:else}
		<div class="discover">
			{#if filtersOpen}
				<button
					class="scrim"
					type="button"
					aria-label="Close filters"
					onclick={() => (filtersOpen = false)}
				></button>
			{/if}
			<aside class="facets" class:open={filtersOpen} aria-label="Filters">
				<div class="drawer-head">
					<h2>Filters</h2>
					<button class="btn ghost sm" type="button" onclick={() => (filtersOpen = false)}
						>Done</button
					>
				</div>
				<WebsitePicker modules={modules ?? []} websites={websites ?? []} bind:selected={module} />
				{#if selected}
					{#key selected.id}
						<ListActions {api} store={events} module={selected} onfinished={afterJob} />
					{/key}
				{:else}
					<p class="small muted">Searching every selected website’s list.</p>
				{/if}
				<fieldset class="group">
					<legend class="group-head">Website filters</legend>
					<p class="small muted">From each website’s list.</p>
					<div class="field">
						<label class="label" for="status">Status</label>
						<select id="status" class="input" bind:value={status}>
							<option value="">Any</option>
							{#each Object.entries(STATUS) as [value, label] (value)}
								{@const count = facets.statuses.find((s) => s.value === value)?.count ?? 0}
								<option {value}>{label} ({count})</option>
							{/each}
						</select>
					</div>
					<GenreChips genres={facets.genres} bind:states={genres} />
				</fieldset>
				{#if mangabaka?.available}
					<fieldset class="group">
						<legend class="group-head">Metadata filters</legend>
						<p class="small muted">From MangaBaka.</p>
						{#if mangabaka.downloaded}
							<div class="field">
								<label class="label" for="format">Format</label>
								<select id="format" class="input" bind:value={format}>
									<option value="">Any</option>
									{#each options(FORMAT, facets.formats) as option (option.value)}
										<option value={option.value}>{option.label} ({option.count})</option>
									{/each}
								</select>
							</div>
							<div class="field">
								<label class="label" for="publication">Publication</label>
								<select id="publication" class="input" bind:value={publication}>
									<option value="">Any</option>
									{#each options(PUBLICATION, facets.publications) as option (option.value)}
										<option value={option.value}>{option.label} ({option.count})</option>
									{/each}
								</select>
							</div>
						{:else}
							<p class="small muted">
								<a href="/settings#section-metadata">Download the MangaBaka database</a> to filter by
								format and publication.
							</p>
						{/if}
					</fieldset>
				{/if}
			</aside>

			<section class="results" aria-label="Results">
				<div class="searchrow">
					<input
						class="input search"
						type="search"
						placeholder="Search titles"
						aria-label="Search titles"
						bind:value={text}
					/>
					<button class="btn filters-btn" type="button" onclick={() => (filtersOpen = true)}
						>Filters</button
					>
				</div>
				<div class="small muted" role="status">
					{#if loading && !items.length}
						Searching…
					{:else}
						<span class="num">{total.toLocaleString('en')}</span>
						{total === 1 ? 'title' : 'titles'}
					{/if}
				</div>
				{#if error}
					<p class="bad" role="alert">{error}</p>
				{/if}

				{#if items.length}
					<ul class="grid">
						{#each items as item (`${item.module_id}\n${item.link}`)}
							<li>
								<a class="card" href={seriesHref(item)}>
									<CoverThumb src={loadCovers ? item.cover_url : null} title={item.title} />
									<span class="t">{item.title}</span>
									<span class="small muted">{subtitle(item)}</span>
								</a>
							</li>
						{/each}
					</ul>
					{#if more}
						<div class="more" use:infinite>
							<button class="btn" type="button" disabled={loading} onclick={loadMore}
								>{loading ? 'Loading…' : 'Load more'}</button
							>
						</div>
					{/if}
				{:else if !loading && !error}
					<p class="muted empty">
						{#if selected && !selected.list_size}
							{selected.name} has no list yet.
						{:else}
							No titles match. Loosen the genre filter or the search.
						{/if}
					</p>
				{/if}
			</section>
		</div>
	{/if}
</div>

<style>
	.discover {
		display: flex;
		gap: 22px;
		align-items: flex-start;
	}
	.facets {
		width: 270px;
		flex: none;
		position: sticky;
		top: 76px;
		display: flex;
		flex-direction: column;
		gap: var(--sp-4);
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		padding: 14px;
		max-height: calc(100vh - 96px);
		overflow: auto;
	}
	.drawer-head,
	.filters-btn,
	.scrim {
		display: none;
	}
	.group {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
		margin: 0;
		padding: var(--sp-4) 0 0;
		border: 0;
		border-top: 1px solid var(--line);
		min-width: 0;
	}
	/* Floated, the legend is laid out as a flex item instead of sitting in the border. */
	.group-head {
		float: left;
		width: 100%;
		padding: 0;
		font-weight: 600;
	}
	.field {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.results {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
	}
	.searchrow {
		display: flex;
		gap: var(--sp-2);
	}
	.search {
		flex: 1;
		font-size: 15px;
		padding: 8px 12px;
	}
	.grid {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
		gap: 20px 16px;
	}
	.card {
		display: flex;
		flex-direction: column;
		gap: 6px;
		color: inherit;
		text-decoration: none;
	}
	.t {
		font-weight: 600;
		font-size: 13.5px;
		line-height: 1.25;
	}
	.more {
		display: flex;
		justify-content: center;
	}
	.empty,
	p {
		margin: 0;
	}
	.bad {
		color: var(--bad);
	}

	@media (max-width: 860px) {
		.facets {
			display: none;
		}
		.facets.open {
			display: flex;
			position: fixed;
			z-index: 45;
			top: 0;
			left: 0;
			bottom: 0;
			width: min(340px, 88vw);
			max-height: none;
			border-radius: 0;
			padding-top: calc(14px + var(--safe-top));
			padding-bottom: calc(14px + var(--safe-bottom));
		}
		.drawer-head {
			display: flex;
			align-items: center;
			justify-content: space-between;
		}
		.drawer-head h2 {
			font-size: var(--fs-lg);
		}
		.filters-btn {
			display: inline-block;
		}
		.scrim {
			display: block;
			position: fixed;
			inset: 0;
			z-index: 44;
			border: 0;
			background: rgba(5, 15, 18, 0.45);
		}
		.grid {
			grid-template-columns: repeat(auto-fill, minmax(110px, 1fr));
		}
	}
</style>
