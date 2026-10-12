<script lang="ts">
	import { onMount } from 'svelte';
	import { SvelteSet } from 'svelte/reactivity';
	import type { Api } from '#lib/api/client.ts';
	import type { MangaBakaSearchResult, SeriesInfo } from '#lib/api/types.ts';

	let {
		api,
		series,
		onmatched,
		onclose
	}: {
		api: Api;
		/** The series to match. */
		series: SeriesInfo;
		/** Called once the picked series is its match. */
		onmatched: () => void;
		onclose: () => void;
	} = $props();

	/** Typing waits this long before searching. */
	const DEBOUNCE_MS = 300;

	const uid = $props.id();
	let dialog: HTMLDialogElement | undefined = $state();
	// The search starts from the series' title; the series itself doesn't change while it's open.
	// svelte-ignore state_referenced_locally
	let query = $state(series.title);
	let results = $state<MangaBakaSearchResult[] | null>(null);
	let searching = $state(false);
	let picking = $state<number | null>(null);
	let error = $state<string | null>(null);
	/** Covers that failed to load, by series ID. */
	const failedCovers = new SvelteSet<number>();
	/** Only the latest search's answer is shown. */
	let searchId = 0;
	let timer: ReturnType<typeof setTimeout> | undefined;

	onMount(() => {
		dialog?.showModal();
		search();
		return () => clearTimeout(timer);
	});

	async function search() {
		clearTimeout(timer);
		const q = query.trim();
		const id = ++searchId;
		if (!q) {
			results = [];
			return;
		}
		searching = true;
		error = null;
		try {
			const found = await api.searchMangabaka(q);
			if (id === searchId) results = found;
		} catch {
			if (id === searchId) error = 'Could not search MangaBaka.';
		} finally {
			if (id === searchId) searching = false;
		}
	}

	function typed() {
		clearTimeout(timer);
		timer = setTimeout(search, DEBOUNCE_MS);
	}

	async function pick(result: MangaBakaSearchResult) {
		picking = result.id;
		error = null;
		try {
			await api.setSeriesMatch(series.module_id, series.link, result.id);
			onmatched();
			dialog?.close();
		} catch {
			error = 'Could not match the series.';
		} finally {
			picking = null;
		}
	}

	const FORMAT: Record<string, string> = {
		manga: 'Manga',
		manhwa: 'Manhwa',
		manhua: 'Manhua',
		oel: 'OEL',
		other: 'Other'
	};
</script>

<dialog class="dialog" bind:this={dialog} aria-labelledby="{uid}-title" {onclose}>
	<header>
		<h2 id="{uid}-title">Match on MangaBaka</h2>
		<button class="btn ghost sm" type="button" onclick={() => dialog?.close()}>Close</button>
	</header>
	<p class="small muted">
		Pick the MangaBaka series that <b>{series.title}</b> is. Search by title, MangaBaka ID or URL.
	</p>
	<form
		class="search"
		onsubmit={(e) => {
			e.preventDefault();
			search();
		}}
	>
		<input
			class="input"
			type="search"
			aria-label="Search MangaBaka"
			autocomplete="off"
			bind:value={query}
			oninput={typed}
		/>
		<button class="btn" type="submit" disabled={searching}>Search</button>
	</form>
	{#if error}
		<p class="error small" role="alert">{error}</p>
	{/if}
	<p class="visually-hidden" aria-live="polite">{searching ? 'Searching…' : ''}</p>
	{#if results}
		{#if results.length === 0 && !searching}
			<p class="small muted">No MangaBaka series found.</p>
		{:else}
			<ul class="results" aria-label="MangaBaka series">
				{#each results as result (result.id)}
					<li>
						<button
							class="result"
							type="button"
							disabled={picking !== null}
							onclick={() => pick(result)}
						>
							{#if result.cover && !failedCovers.has(result.id)}
								<img
									class="cover"
									src={result.cover}
									alt=""
									loading="lazy"
									onerror={() => failedCovers.add(result.id)}
								/>
							{:else}
								<span class="cover blank" aria-hidden="true"></span>
							{/if}
							<span class="text">
								<span class="title">{result.title}</span>
								{#if result.english_title && result.english_title !== result.title}
									<span class="small muted">{result.english_title}</span>
								{/if}
								<span class="small muted">
									{[result.year, FORMAT[result.format] ?? result.format]
										.filter((v) => v != null)
										.join(' · ')}
								</span>
							</span>
							{#if picking === result.id}
								<span class="small muted">Matching…</span>
							{/if}
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	{/if}
</dialog>

<style>
	.dialog {
		width: min(560px, calc(100vw - 32px));
		max-height: min(720px, calc(100vh - 32px));
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		background: var(--surface);
		color: var(--fg);
		box-shadow: var(--shadow);
		padding: var(--sp-4);
	}
	.dialog[open] {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
	}
	.dialog::backdrop {
		background: var(--scrim);
	}
	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--sp-2);
	}
	h2 {
		margin: 0;
		font-size: var(--fs-lg);
	}
	p {
		margin: 0;
	}
	.search {
		display: flex;
		gap: var(--sp-2);
	}
	.search .input {
		flex: 1;
		min-width: 0;
	}
	.results {
		list-style: none;
		margin: 0;
		padding: 0;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
	}
	.result {
		width: 100%;
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		padding: var(--sp-2);
		border: 1px solid transparent;
		border-radius: var(--r);
		background: none;
		color: inherit;
		font: inherit;
		text-align: left;
		cursor: pointer;
	}
	.result:hover:not(:disabled),
	.result:focus-visible {
		border-color: var(--line);
		background: var(--surface-2);
	}
	.result:disabled {
		cursor: default;
	}
	.cover {
		width: 40px;
		height: 57px;
		flex: none;
		border-radius: var(--r);
		object-fit: cover;
		background: var(--surface-2);
	}
	.cover.blank {
		background: var(--cover-placeholder);
	}
	.text {
		display: flex;
		flex-direction: column;
		min-width: 0;
		flex: 1;
		overflow-wrap: anywhere;
	}
	.title {
		font-weight: 600;
	}
	.error {
		color: var(--bad);
	}
</style>
