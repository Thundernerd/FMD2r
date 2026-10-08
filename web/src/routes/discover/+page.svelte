<script lang="ts">
	import type { ListFacets, ListItem, ModuleSummary } from '#lib/api/types.ts';
	import { api, events } from '#lib/app.ts';
	import GenreChips from '#lib/components/discover/GenreChips.svelte';
	import ListActions from '#lib/components/discover/ListActions.svelte';
	import WebsitePicker from '#lib/components/discover/WebsitePicker.svelte';
	import {
		emptyFilters,
		facetQuery,
		searchQuery,
		type Filters,
		type Tri
	} from '#lib/discover/filters.ts';

	/** How long typing pauses before the search runs. */
	const DEBOUNCE_MS = 250;
	/** `MangaInfo_Status*` (baseunits/uBaseUnit.pas:230-233). */
	const STATUS: Record<string, string> = {
		'0': 'Completed',
		'1': 'Ongoing',
		'2': 'Hiatus',
		'3': 'Cancelled'
	};

	let modules = $state<ModuleSummary[]>([]);
	let module = $state('');
	let text = $state('');
	let q = $state('');
	let genres = $state<Record<string, Tri>>({});
	let status = $state('');
	let filtersOpen = $state(false);

	let items = $state<ListItem[]>([]);
	let total = $state(0);
	let page = $state(1);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let facets = $state<ListFacets>({ genres: [], statuses: [] });

	const selected = $derived(modules.find((m) => m.id === module));
	const names = $derived(new Map(modules.map((m) => [m.id, m.name])));
	const filters = $derived<Filters>({ module, q, genres, status, page: 1 });
	const more = $derived(items.length < total);

	function loadModules() {
		api
			.listModules()
			.then((list) => (modules = list))
			.catch(() => (error = 'Could not load the websites.'));
	}
	$effect(loadModules);

	$effect(() => {
		const value = text;
		const timer = setTimeout(() => (q = value), DEBOUNCE_MS);
		return () => clearTimeout(timer);
	});

	/** Bumped by every new search, so answers to an older one are dropped. */
	let generation = 0;

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
			.catch(() => (facets = { genres: [], statuses: [] }));
	}

	function reload() {
		void load(1, true);
		loadFacets();
	}

	$effect(() => {
		// A new search whenever a filter changes.
		void filters;
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

	function seriesHref(item: ListItem): string {
		const link = item.link.replace(/^\/+/, '').split('/').map(encodeURIComponent).join('/');
		return `/series/${encodeURIComponent(item.module_id)}/${link}`;
	}

	/** The website and status under a title. */
	function subtitle(item: ListItem): string {
		const site = names.get(item.module_id) ?? item.module_id;
		const status = STATUS[item.status];
		return status ? `${site} · ${status}` : site;
	}

	/** A stable hue per title for its placeholder cover. */
	function hue(title: string): number {
		let h = 0;
		for (const c of title) h = (h * 31 + c.charCodeAt(0)) % 360;
		return h;
	}

	function afterJob() {
		loadModules();
		reload();
	}
</script>

<svelte:head><title>Discover · FMD2r</title></svelte:head>

<div class="page">
	<h1>Discover</h1>

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
			<WebsitePicker {modules} bind:selected={module} />
			{#if selected}
				{#key selected.id}
					<ListActions {api} store={events} module={selected} onfinished={afterJob} />
				{/key}
			{:else}
				<p class="small muted">Searching every website’s list.</p>
			{/if}
			<div class="status">
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
								<span class="cover" style:--h={hue(item.title)} aria-hidden="true"
									>{item.title}</span
								>
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
	.status {
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
	.card:hover .cover {
		outline: 2px solid var(--accent);
		outline-offset: 2px;
	}
	.cover {
		width: 100%;
		aspect-ratio: 5 / 7;
		display: flex;
		align-items: flex-end;
		padding: 6px;
		border-radius: 4px;
		color: #fff;
		font: 700 13px/1.1 var(--f-display);
		background: linear-gradient(160deg, hsl(var(--h) 45% 52%), hsl(calc(var(--h) + 40) 50% 28%));
		text-shadow: 0 1px 2px rgba(0, 0, 0, 0.4);
		overflow: hidden;
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
