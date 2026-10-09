<script lang="ts">
	import { ApiError, type Api } from '#lib/api/client.ts';
	import type { SeriesInfo, SeriesStatus } from '#lib/api/types.ts';

	let {
		api,
		series = $bindable(),
		website
	}: { api: Api; series: SeriesInfo; website: string } = $props();

	let adding = $state(false);
	let addError = $state<string | null>(null);

	let checking = $state(false);
	let checkNote = $state<string | null>(null);

	/** Starts a check of this series for chapters missing from its folder. */
	async function checkMissing() {
		checking = true;
		checkNote = null;
		try {
			const favorite = (await api.listFavorites()).find(
				(f) => f.module_id === series.module_id && f.link === series.link
			);
			if (!favorite) throw new Error('not in the library');
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
			await api.addFavorite(series.module_id, series.link);
			series.in_library = true;
		} catch (e) {
			if (e instanceof ApiError && e.status === 409) series.in_library = true;
			else addError = e instanceof ApiError && e.detail ? e.detail : 'Could not add it.';
		} finally {
			adding = false;
		}
	}

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
			<div>
				<dt class="label">Chapters</dt>
				<dd class="num">{series.chapters.length} · {seen} seen</dd>
			</div>
		</dl>
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
				<button class="btn" type="button" disabled={checking} onclick={checkMissing}>
					Check missing chapters
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
		</div>
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
		color: #fff;
		font: 700 18px/1.1 var(--f-display);
		background: linear-gradient(160deg, #3f8f9c, #173e46);
		text-shadow: 0 1px 2px rgba(0, 0, 0, 0.4);
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
		font-size: 34px;
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
		font-size: 13px;
	}
	.facts div {
		display: flex;
		flex-direction: column;
	}
	.facts dd {
		margin: 0;
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
			font-size: 26px;
		}
	}
</style>
