<!-- PROTOTYPE (#224): the inbox inline instead of in a popover (C's home, R's rail). -->
<script lang="ts">
	import type { Api } from '#lib/api/client.ts';
	import type { EventStore } from '#lib/events.svelte.ts';

	let {
		api,
		store,
		unreadOnly = false
	}: { api: Api; store: EventStore; unreadOnly?: boolean } = $props();

	const items = $derived(unreadOnly ? store.inbox.filter((i) => !i.read) : store.inbox);

	async function markRead(id: string) {
		await api.markRead(id).catch(() => {});
		store.markRead(id);
	}
</script>

{#if items.length === 0}
	<p class="small muted empty">Nothing needs you.</p>
{:else}
	<ul class="ilist">
		{#each items as item (item.id)}
			<li class="item kind-{item.kind}" class:unread={!item.read}>
				<b class="small">{item.title}</b>
				<span class="small muted body">{item.body.split(/\r?\n/)[0]}</span>
				{#if !item.read}
					<button class="btn sm ghost" type="button" onclick={() => markRead(item.id)}
						>Mark read</button
					>
				{/if}
			</li>
		{/each}
	</ul>
{/if}

<style>
	.ilist {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.item {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 2px;
		padding: var(--sp-2) var(--sp-3);
		border-radius: var(--r);
		background: var(--surface);
		border: 1px solid var(--line);
		border-left: 3px solid var(--line);
	}
	.item.unread.kind-warn {
		border-left-color: var(--warn);
	}
	.item.unread.kind-error {
		border-left-color: var(--bad);
	}
	.item.unread.kind-info {
		border-left-color: var(--accent);
	}
	.item:not(.unread) {
		color: var(--muted);
	}
	.body {
		overflow-wrap: anywhere;
	}
	.empty {
		margin: 0;
	}
</style>
