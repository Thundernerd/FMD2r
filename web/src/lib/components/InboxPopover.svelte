<script lang="ts">
	import type { Api } from '#lib/api/client.ts';
	import type { InboxItem } from '#lib/api/types.ts';
	import type { EventStore } from '#lib/events.svelte.ts';
	import { inboxUi } from '#lib/inbox.svelte.ts';

	let { api, store }: { api: Api; store: EventStore } = $props();

	let pop: HTMLElement | undefined = $state();
	let error = $state<string | null>(null);
	let root: HTMLElement | undefined = $state();

	const formatTime = (iso: string): string =>
		new Date(iso).toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' });

	async function markRead(item: InboxItem) {
		error = null;
		try {
			await api.markRead(item.id);
			store.markRead(item.id);
		} catch {
			error = 'Could not mark the item read.';
		}
	}

	function close() {
		inboxUi.open = false;
		inboxUi.focus = null;
	}

	function onWindowClick(event: MouseEvent) {
		if (inboxUi.open && root && event.target instanceof Node && !root.contains(event.target)) {
			close();
		}
	}

	function onKeydown(event: KeyboardEvent) {
		if (inboxUi.open && event.key === 'Escape') close();
	}

	// Bring the item another page pointed at into view.
	$effect(() => {
		if (!inboxUi.open || !inboxUi.focus || !pop) return;
		pop.querySelector('.focused')?.scrollIntoView({ block: 'nearest' });
	});
</script>

<svelte:window onclick={onWindowClick} onkeydown={onKeydown} />

<div class="inbox" bind:this={root}>
	<button
		class="bell"
		type="button"
		aria-expanded={inboxUi.open}
		aria-haspopup="dialog"
		onclick={() => (inboxUi.open ? close() : (inboxUi.open = true))}
	>
		Inbox
		{#if store.unread > 0}
			<b class="badge" aria-label="{store.unread} unread">{store.unread}</b>
		{/if}
	</button>

	{#if inboxUi.open}
		<div class="pop" role="dialog" aria-label="Inbox" bind:this={pop}>
			{#if store.inbox.length === 0}
				<p class="empty muted">Nothing needs you.</p>
			{:else}
				<ul>
					{#each store.inbox as item (item.id)}
						<li
							class="item kind-{item.kind}"
							class:unread={!item.read}
							class:focused={item.id === inboxUi.focus}
						>
							<div class="head">
								<b class="title">{item.title}</b>
								<span class="small muted">{formatTime(item.created_at)}</span>
							</div>
							<div class="body small">
								{#each item.body.split(/\r?\n/) as line, i (i)}
									<div class="line">{line}</div>
								{/each}
							</div>
							{#if !item.read}
								<div>
									<button class="btn sm" type="button" onclick={() => markRead(item)}>
										Mark read
									</button>
								</div>
							{/if}
						</li>
					{/each}
				</ul>
			{/if}
			{#if error}
				<p class="error small" role="alert">{error}</p>
			{/if}
		</div>
	{/if}
</div>

<style>
	.inbox {
		position: relative;
		flex: none;
	}
	.bell {
		position: relative;
		border: 1px solid var(--line);
		background: var(--surface);
		border-radius: var(--r-pill);
		padding: 5px 12px;
	}
	.badge {
		position: absolute;
		top: -5px;
		right: -5px;
		background: var(--warn);
		color: var(--surface);
		border-radius: var(--r-pill);
		font-size: 10.5px;
		padding: 1px 6px;
	}
	.pop {
		position: absolute;
		right: 0;
		top: calc(100% + 10px);
		width: 420px;
		max-width: calc(100vw - 32px);
		max-height: 70vh;
		overflow: auto;
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		box-shadow: var(--shadow);
		z-index: 30;
		padding: 6px;
	}
	ul {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	.item {
		padding: 10px;
		border-bottom: 1px solid var(--line);
		border-left: 3px solid transparent;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.item:last-child {
		border-bottom: 0;
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
	.item.focused {
		background: var(--accent-soft);
	}
	.item:not(.unread) {
		color: var(--muted);
	}
	.kind-error .title {
		color: var(--bad);
	}
	.head {
		display: flex;
		gap: var(--sp-2);
		align-items: baseline;
	}
	.title {
		flex: 1;
		min-width: 0;
	}
	/* Keep a stack trace's indentation, and wrap long lines instead of overflowing. */
	.line {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		min-height: 1lh;
	}
	.empty,
	.error {
		margin: 0;
		padding: 10px;
	}
	.error {
		color: var(--bad);
	}
</style>
