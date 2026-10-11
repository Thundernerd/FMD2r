<!-- PROTOTYPE (#224): C's home page, "what needs me today" instead of the cover grid. -->
<script lang="ts">
	import type { Api } from '#lib/api/client.ts';
	import type { FavoriteView } from '#lib/api/types.ts';
	import type { EventStore } from '#lib/events.svelte.ts';
	import { seriesHref } from '#lib/series/href.ts';
	import InboxList from './InboxList.svelte';
	import QueueMini from './QueueMini.svelte';

	let { api, events }: { api: Api; events: EventStore } = $props();

	let favorites = $state<FavoriteView[]>([]);
	$effect(() => {
		api
			.listFavorites()
			.then((list) => (favorites = list))
			.catch(() => {});
	});
	const fresh = $derived(favorites.filter((f) => f.new_chapters > 0));
</script>

<div class="page">
	<h1>Today</h1>
	<p class="muted">
		Type <kbd>/</kbd> to add a URL, open a series or search all websites. Your
		<a href="/?library">library</a> is one keystroke away.
	</p>
	<div class="cols">
		<section class="col">
			<h2>Needs you <span class="muted num">{events.unread}</span></h2>
			<InboxList {api} store={events} unreadOnly />
		</section>
		<section class="col">
			<h2>New chapters <span class="muted num">{fresh.length}</span></h2>
			{#each fresh as f (f.id)}
				<a class="row" href={seriesHref(f)}>
					<span>{f.title}</span><b class="small num new">+{f.new_chapters}</b>
				</a>
			{:else}
				<p class="small muted">No new chapters.</p>
			{/each}
		</section>
		<section class="col">
			<h2>Downloading</h2>
			<QueueMini queue={events.queue} graph waitingList />
		</section>
	</div>
</div>

<style>
	p {
		margin: 0;
	}
	kbd {
		font-family: var(--f-mono);
		padding: 0 4px;
		border: 1px solid var(--line);
		border-radius: var(--r);
	}
	.cols {
		display: flex;
		flex-direction: column;
		gap: var(--sp-5);
	}
	.col {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	h2 {
		font-size: var(--fs-lg);
	}
	.row {
		display: flex;
		justify-content: space-between;
		gap: var(--sp-2);
		padding: var(--sp-2) var(--sp-3);
		border-radius: var(--r);
		background: var(--surface);
		border: 1px solid var(--line);
		color: var(--fg);
		text-decoration: none;
	}
	.row:hover {
		border-color: var(--accent);
	}
	.new {
		color: var(--ok);
	}
	@media (min-width: 861px) {
		.cols {
			flex-direction: row;
		}
	}
</style>
