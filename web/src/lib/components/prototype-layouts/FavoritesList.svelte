<!-- PROTOTYPE (#224): W's favorites list pane; the selected one opens in the detail pane. -->
<script lang="ts">
	import { page } from '$app/state';
	import type { Api } from '#lib/api/client.ts';
	import type { FavoriteView } from '#lib/api/types.ts';
	import { seriesHref } from '#lib/series/href.ts';

	let { api }: { api: Api } = $props();

	let favorites = $state<FavoriteView[]>([]);
	let text = $state('');
	$effect(() => {
		api
			.listFavorites()
			.then((list) => (favorites = list))
			.catch(() => {});
	});

	const shown = $derived(
		favorites.filter((f) => f.title.toLowerCase().includes(text.trim().toLowerCase()))
	);
	const isOpen = (f: FavoriteView): boolean =>
		page.url.pathname === '/series' &&
		page.url.searchParams.get('module') === f.module_id &&
		page.url.searchParams.get('link') === f.link;
</script>

<div class="flist">
	<div class="head">
		<a class="all small" href="/" aria-current={page.url.pathname === '/' ? 'page' : undefined}
			>All favorites <span class="muted num">{favorites.length}</span></a
		>
		<input class="input" type="search" placeholder="Filter" aria-label="Filter" bind:value={text} />
	</div>
	<ul>
		{#each shown as f (f.id)}
			<li>
				<a class="row" class:open={isOpen(f)} href={seriesHref(f)}>
					<span class="t">{f.title}</span>
					{#if f.new_chapters > 0}<b class="new small num">+{f.new_chapters}</b>{/if}
					<span class="small muted site">{f.website}</span>
				</a>
			</li>
		{/each}
	</ul>
</div>

<style>
	.flist {
		display: flex;
		flex-direction: column;
		min-height: 0;
		height: 100%;
	}
	.head {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
		padding: var(--sp-3);
		border-bottom: 1px solid var(--line);
	}
	.all {
		color: var(--fg);
		text-decoration: none;
		font-weight: 600;
	}
	.all[aria-current='page'] {
		color: var(--accent);
	}
	ul {
		list-style: none;
		margin: 0;
		padding: var(--sp-1);
		overflow: auto;
		flex: 1;
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		gap: 0 var(--sp-2);
		padding: 6px var(--sp-2);
		border-radius: var(--r);
		color: var(--fg);
		text-decoration: none;
		font-size: var(--fs-ui);
	}
	.row:hover {
		background: var(--surface-2);
	}
	.row.open {
		background: var(--accent-soft);
		color: var(--accent);
	}
	.t {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.new {
		color: var(--ok);
	}
	.site {
		flex-basis: 100%;
	}
</style>
