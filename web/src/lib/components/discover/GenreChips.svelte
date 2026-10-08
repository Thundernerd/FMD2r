<script lang="ts">
	import type { FacetValue } from '#lib/api/types.ts';
	import { cycle, type Tri } from '#lib/discover/filters.ts';

	let {
		genres,
		states = $bindable()
	}: {
		/** The genres of the titles the search matches, with their counts. */
		genres: FacetValue[];
		states: Record<string, Tri>;
	} = $props();

	/** The facet genres, plus chosen genres the current facets no longer hold. */
	const chips = $derived([
		...genres,
		...Object.keys(states)
			.filter((g) => states[g] !== 'ignore' && !genres.some((f) => f.value === g))
			.map((value) => ({ value, count: 0 }))
	]);
	const active = $derived(Object.values(states).some((s) => s !== 'ignore'));

	const LABEL: Record<Tri, string> = {
		ignore: 'ignored',
		include: 'included',
		exclude: 'excluded'
	};

	function toggle(genre: string) {
		const next = cycle(states[genre] ?? 'ignore');
		const rest = Object.fromEntries(Object.entries(states).filter(([g]) => g !== genre));
		states = next === 'ignore' ? rest : { ...rest, [genre]: next };
	}
</script>

<div class="genres">
	<div class="head">
		<span class="label">Genres</span>
		<span class="small muted hint">click to include, again to exclude</span>
		{#if active}
			<button class="btn ghost sm" type="button" onclick={() => (states = {})}>Clear</button>
		{/if}
	</div>
	{#if chips.length}
		<div class="chips" role="group" aria-label="Genres">
			{#each chips as genre (genre.value)}
				{@const state = states[genre.value] ?? 'ignore'}
				<button
					class="tri {state}"
					type="button"
					aria-label="{genre.value}: {LABEL[state]}"
					title="Click: include → exclude → ignore"
					onclick={() => toggle(genre.value)}
				>
					{#if state === 'include'}+{:else if state === 'exclude'}−{/if}{genre.value}
					<span class="count num">{genre.count}</span>
				</button>
			{/each}
		</div>
	{:else}
		<p class="small muted">No genres listed.</p>
	{/if}
</div>

<style>
	.genres {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.head {
		display: flex;
		align-items: baseline;
		gap: var(--sp-2);
		flex-wrap: wrap;
	}
	.hint {
		flex: 1;
	}
	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: 5px;
	}
	.tri {
		display: inline-flex;
		align-items: center;
		gap: var(--sp-1);
		border: 1px solid var(--line);
		border-radius: var(--r-pill);
		padding: 1px 9px;
		font-size: var(--fs-sm);
		background: var(--surface);
	}
	.tri:hover {
		border-color: var(--accent);
	}
	.tri.include {
		background: var(--ok-soft);
		color: var(--ok);
		border-color: transparent;
	}
	.tri.exclude {
		background: var(--bad-soft);
		color: var(--bad);
		border-color: transparent;
		text-decoration: line-through;
	}
	.count {
		color: var(--muted);
		font-size: var(--fs-xs);
	}
	p {
		margin: 0;
	}
</style>
