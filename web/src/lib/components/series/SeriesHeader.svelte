<script lang="ts">
	import { ApiError, type Api } from '#lib/api/client.ts';
	import type { FavoriteView, SeriesInfo, SeriesStatus } from '#lib/api/types.ts';
	import { FORMAT } from '#lib/discover/filters.ts';
	import { untrack } from 'svelte';
	import FolderDialog from './FolderDialog.svelte';
	import MangaBakaMatchDialog from './MangaBakaMatchDialog.svelte';

	let {
		api,
		series = $bindable(),
		website,
		saveTo = ''
	}: {
		api: Api;
		series: SeriesInfo;
		website: string;
		/** The folder picked in the download box, which the series keeps in the library. */
		saveTo?: string;
	} = $props();

	let adding = $state(false);
	let addError = $state<string | null>(null);

	let checking = $state(false);
	let checkNote = $state<string | null>(null);

	/** This series in the library, loaded with the page. */
	let favorite = $state<FavoriteView | null>(null);
	let loadError = $state<string | null>(null);
	let folderOpen = $state(false);
	let folderError = $state<string | null>(null);

	/** This series' entry in the library. */
	async function findFavorite(): Promise<FavoriteView> {
		const found = (await api.listFavorites()).find(
			(f) => f.module_id === series.module_id && f.link === series.link
		);
		if (!found) throw new Error('not in the library');
		return found;
	}

	/**
	 * Which series' library entry to load; null for a series not in the library. A string, so the
	 * entry loads again only when the series joins the library, not when its info reloads.
	 */
	const libraryKey = $derived(series.in_library ? `${series.module_id}\n${series.link}` : null);
	$effect(() => {
		if (libraryKey === null) return;
		untrack(async () => {
			loadError = null;
			try {
				favorite = await findFavorite();
			} catch {
				loadError = 'Could not load this series from the library.';
			}
		});
	});

	/** A series the check skips: its favorite is disabled. */
	const paused = $derived(favorite !== null && !favorite.enabled);
	let pausing = $state(false);
	let pauseError = $state<string | null>(null);

	/** Pauses checking this series for new chapters, or resumes it. */
	async function togglePaused() {
		if (!favorite) return;
		pausing = true;
		pauseError = null;
		try {
			favorite = await api.updateFavorite(favorite.id, { enabled: !favorite.enabled });
		} catch {
			pauseError = paused ? 'Could not resume checking.' : 'Could not pause checking.';
		} finally {
			pausing = false;
		}
	}

	/** Opens the dialog that changes where new chapters of this series download. */
	async function openFolderDialog() {
		folderError = null;
		try {
			favorite ??= await findFavorite();
			folderOpen = true;
		} catch {
			folderError = 'Could not load the download folder.';
		}
	}

	/** Starts a check of this series for chapters missing from its folder. */
	async function checkMissing() {
		if (!favorite) return;
		checking = true;
		checkNote = null;
		try {
			await api.checkMissingChapters(favorite.id);
			checkNote = 'Checking for missing chapters; the result arrives in the inbox.';
		} catch (e) {
			checkNote =
				e instanceof ApiError && e.status === 409
					? 'A check is already running.'
					: e instanceof ApiError && e.status === 503
						? 'The chapter check is not running on this server.'
						: 'Could not start the check.';
		} finally {
			checking = false;
		}
	}

	async function addToLibrary() {
		adding = true;
		addError = null;
		try {
			await api.addFavorite(series.module_id, series.link, saveTo);
			series.in_library = true;
		} catch (e) {
			if (e instanceof ApiError && e.status === 409) series.in_library = true;
			else addError = e instanceof ApiError && e.detail ? e.detail : 'Could not add it.';
		} finally {
			adding = false;
		}
	}

	let matchOpen = $state(false);
	let matchBusy = $state(false);
	let matchError = $state<string | null>(null);

	/** Loads the series again, so what MangaBaka says of it follows its new match. */
	async function reload() {
		matchError = null;
		try {
			series = await api.getSeries(series.module_id, series.link);
		} catch {
			matchError = 'Could not load the series again; reload the page.';
		}
	}

	/** Undoes the match by hand; the series is matched automatically again. */
	async function undoMatch() {
		matchBusy = true;
		matchError = null;
		try {
			await api.removeSeriesMatch(series.module_id, series.link);
			await reload();
		} catch {
			matchError = 'Could not undo the match.';
		} finally {
			matchBusy = false;
		}
	}

	const mangabakaUrl = $derived(
		series.mangabaka
			? (series.mangabaka.links.find((l) => l.label === 'MangaBaka')?.url ??
					`https://mangabaka.org/${series.mangabaka.id}`)
			: null
	);

	const STATUS: Record<SeriesStatus, string> = {
		ongoing: 'Ongoing',
		completed: 'Completed',
		hiatus: 'Hiatus',
		cancelled: 'Cancelled',
		unknown: 'Status unknown'
	};

	let coverFailed = $state(false);
	let expanded = $state(false);
	// Short summaries show in full; longer ones start clamped.
	const long = $derived(series.summary.length > 280 || series.summary.split('\n').length > 4);
	const seen = $derived(series.chapters.filter((c) => c.downloaded).length);

	/** Links shown before "Show all". */
	const FEW_LINKS = 8;
	let allLinks = $state(false);
	// The website first, then what MangaBaka knows of the series.
	const links = $derived([{ label: website, url: series.url }, ...(series.mangabaka?.links ?? [])]);
</script>

<header class="hero">
	{#if series.cover_url && !coverFailed}
		<img class="cover" src={series.cover_url} alt="" onerror={() => (coverFailed = true)} />
	{:else}
		<div class="cover blank" aria-hidden="true">{series.title}</div>
	{/if}
	<div class="meta">
		<div class="kicker">
			<span class="label">{website}</span>
			<span class="chip status-{series.status}">{STATUS[series.status]}</span>
		</div>
		<h1>{series.title}</h1>
		{#if series.alt_titles}
			<p class="muted alt">{series.alt_titles}</p>
		{/if}
		<dl class="facts">
			{#if series.authors}
				<div>
					<dt class="label">Author</dt>
					<dd>{series.authors}</dd>
				</div>
			{/if}
			{#if series.artists && series.artists !== series.authors}
				<div>
					<dt class="label">Artist</dt>
					<dd>{series.artists}</dd>
				</div>
			{/if}
			{#if series.format}
				<div>
					<dt class="label">Format</dt>
					<dd>{FORMAT[series.format] ?? series.format}</dd>
				</div>
			{/if}
			{#if series.year != null}
				<div>
					<dt class="label">Year</dt>
					<dd class="num">{series.year}</dd>
				</div>
			{/if}
			<div>
				<dt class="label">Chapters</dt>
				<dd class="num">{series.chapters.length} · {seen} seen</dd>
			</div>
			<div class="links-row">
				<dt class="label">Links</dt>
				<dd class="links">
					{#each allLinks ? links : links.slice(0, FEW_LINKS) as link (link.url)}
						<a href={link.url} target="_blank" rel="noreferrer">{link.label}</a>
					{/each}
					{#if !allLinks && links.length > FEW_LINKS}
						<button class="btn sm ghost" type="button" onclick={() => (allLinks = true)}
							>Show all</button
						>
					{/if}
				</dd>
			</div>
		</dl>
		{#if series.mangabaka_match !== 'unavailable'}
			<div class="small match" role="group" aria-label="MangaBaka match">
				{#if series.mangabaka_match === 'unmatched'}
					<span class="muted">Not matched on MangaBaka</span>
				{:else if series.mangabaka_match === 'missing'}
					<span class="warn">The MangaBaka series picked by hand is no longer in the database</span>
				{:else if mangabakaUrl}
					<span
						>Matched on <a href={mangabakaUrl} target="_blank" rel="noreferrer"
							>MangaBaka #{series.mangabaka?.id}</a
						></span
					>
				{/if}
				<span class="muted" aria-hidden="true">·</span>
				<button
					class="btn sm ghost"
					type="button"
					disabled={matchBusy}
					onclick={() => (matchOpen = true)}
				>
					{series.mangabaka_match === 'automatic' || series.mangabaka_match === 'manual'
						? 'Change…'
						: 'Match…'}
				</button>
				{#if series.mangabaka_match === 'manual'}
					<span class="muted" aria-hidden="true">·</span>
					<span class="muted">matched by hand</span>
					<button class="btn sm ghost" type="button" disabled={matchBusy} onclick={undoMatch}>
						Undo
					</button>
				{/if}
				{#if matchError}
					<span class="bad" role="alert">{matchError}</span>
				{/if}
			</div>
		{/if}
		{#if matchOpen}
			<MangaBakaMatchDialog {api} {series} onmatched={reload} onclose={() => (matchOpen = false)} />
		{/if}
		{#if series.genres.length}
			<ul class="genres" aria-label="Genres">
				{#each series.genres as genre (genre)}
					<li class="genre">{genre}</li>
				{/each}
			</ul>
		{/if}
		{#if series.summary}
			<div class="summary">
				<p class:clamped={long && !expanded}>{series.summary}</p>
				{#if series.summary_from_mangabaka}
					<p class="small muted source">from MangaBaka</p>
				{/if}
				{#if long}
					<button class="btn sm ghost" type="button" onclick={() => (expanded = !expanded)}>
						{expanded ? 'Show less' : 'Show more'}
					</button>
				{/if}
			</div>
		{/if}
		<div class="actions">
			{#if series.in_library}
				<span class="btn in-library">★ In library</span>
				{#if favorite}
					<button class="btn" type="button" disabled={pausing} onclick={togglePaused}>
						{paused ? 'Resume checking' : 'Pause checking'}
					</button>
				{/if}
				<button
					class="btn"
					type="button"
					disabled={checking || !favorite || paused}
					title={paused ? 'Resume checking to look for missing chapters' : undefined}
					onclick={checkMissing}
				>
					Check missing chapters
				</button>
				<button class="btn" type="button" title={favorite?.save_to} onclick={openFolderDialog}>
					Download folder…
				</button>
			{:else}
				<button class="btn" type="button" disabled={adding} onclick={addToLibrary}>
					{adding ? 'Adding…' : '＋ Add to library'}
				</button>
			{/if}
			{#if checkNote}
				<span class="small muted note" role="status">{checkNote}</span>
			{/if}
			{#if addError}
				<span class="bad small" role="alert">{addError}</span>
			{/if}
			{#if folderError}
				<span class="bad small" role="alert">{folderError}</span>
			{/if}
			{#if loadError}
				<span class="bad small" role="alert">{loadError}</span>
			{/if}
			{#if pauseError}
				<span class="bad small" role="alert">{pauseError}</span>
			{/if}
		</div>
		{#if series.in_library && paused}
			<p class="small muted paused">
				Paused: not checked for new chapters, so nothing new downloads.
			</p>
		{/if}
		{#if folderOpen && favorite}
			<FolderDialog
				{api}
				{favorite}
				onsaved={(updated) => (favorite = updated)}
				onclose={() => (folderOpen = false)}
			/>
		{/if}
	</div>
</header>

<style>
	.hero {
		display: flex;
		gap: 28px;
		flex-wrap: wrap;
	}
	.cover {
		width: 180px;
		height: 255px;
		flex: none;
		border-radius: var(--r);
		object-fit: cover;
		background: var(--surface-2);
		box-shadow: var(--shadow);
	}
	.cover.blank {
		display: flex;
		align-items: flex-end;
		padding: 10px;
		color: var(--on-cover);
		font: 700 var(--fs-xl)/1.1 var(--f-display);
		background: var(--cover-placeholder);
		text-shadow: var(--on-cover-shadow);
		overflow: hidden;
	}
	.meta {
		flex: 1 1 340px;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.kicker {
		display: flex;
		gap: var(--sp-2);
		align-items: center;
		flex-wrap: wrap;
	}
	h1 {
		font-size: var(--fs-hero);
		letter-spacing: -0.02em;
		overflow-wrap: anywhere;
	}
	.alt {
		margin: 0;
	}
	.chip {
		border-radius: var(--r-pill);
		padding: 1px 8px;
		font-size: var(--fs-xs);
		font-weight: 600;
		background: var(--idle-soft);
		color: var(--idle);
	}
	.chip.status-ongoing {
		background: var(--accent-soft);
		color: var(--accent);
	}
	.chip.status-completed {
		background: var(--ok-soft);
		color: var(--ok);
	}
	.chip.status-hiatus {
		background: var(--warn-soft);
		color: var(--warn);
	}
	.chip.status-cancelled {
		background: var(--bad-soft);
		color: var(--bad);
	}
	.facts {
		display: flex;
		gap: 22px;
		flex-wrap: wrap;
		margin: 0;
		font-size: var(--fs-ui);
	}
	.facts div {
		display: flex;
		flex-direction: column;
	}
	.facts dd {
		margin: 0;
	}
	.facts .links-row {
		flex-basis: 100%;
	}
	.links {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--sp-1) var(--sp-3);
		overflow-wrap: anywhere;
	}
	.match {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--sp-1) var(--sp-2);
	}
	.match .warn {
		color: var(--warn);
	}
	.genres {
		display: flex;
		gap: 6px;
		flex-wrap: wrap;
		list-style: none;
		margin: 0;
		padding: 0;
	}
	.genre {
		border: 1px solid var(--line);
		background: var(--surface);
		border-radius: var(--r-pill);
		padding: 2px 10px;
		font-size: var(--fs-sm);
	}
	.summary {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: var(--sp-1);
		max-width: 65ch;
	}
	.summary p {
		margin: 0;
		white-space: pre-line;
	}
	.summary p.source {
		margin-top: var(--sp-1);
	}
	.summary p.clamped {
		display: -webkit-box;
		-webkit-line-clamp: 4;
		line-clamp: 4;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}
	.actions {
		display: flex;
		gap: var(--sp-2);
		flex-wrap: wrap;
	}
	.bad,
	.note {
		align-self: center;
	}
	.bad {
		color: var(--bad);
	}
	.paused {
		margin: 0;
	}
	.in-library {
		color: var(--accent);
		cursor: default;
	}
	@media (max-width: 860px) {
		.hero {
			gap: var(--sp-4);
		}
		.cover {
			width: 120px;
			height: 170px;
		}
		h1 {
			font-size: var(--fs-h2);
		}
	}
</style>
