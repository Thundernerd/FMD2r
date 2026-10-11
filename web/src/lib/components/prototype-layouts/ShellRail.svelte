<!-- PROTOTYPE (#224) variant R "Queue rail": B's top bar, queue and inbox in a right rail. -->
<script lang="ts">
	import { page } from '$app/state';
	import type { Snippet } from 'svelte';
	import type { Api } from '#lib/api/client.ts';
	import type { EventStore } from '#lib/events.svelte.ts';
	import { formatRate } from '#lib/queue-format.ts';
	import AddByUrl from '#lib/components/AddByUrl.svelte';
	import TopNav from '#lib/components/TopNav.svelte';
	import InboxList from './InboxList.svelte';
	import QueueMini from './QueueMini.svelte';

	let {
		api,
		events,
		banner,
		logout,
		children
	}: {
		api: Api;
		events: EventStore;
		banner: Snippet;
		logout: (() => void) | null;
		children: Snippet;
	} = $props();

	let tab = $state<'queue' | 'inbox'>('queue');
	let sheet = $state(false);
	$effect(() => {
		void page.url.pathname;
		sheet = false;
	});
</script>

<div class="r-app">
	{@render banner()}
	<TopNav>
		<AddByUrl {api} />
		{#if logout}
			<button class="btn ghost sm" type="button" onclick={logout}>Log out</button>
		{/if}
	</TopNav>
	<div class="r-body">
		<main>{@render children()}</main>
		<aside class="r-rail" class:sheet aria-label="Activity">
			<div class="tabs" role="tablist">
				<button
					type="button"
					role="tab"
					aria-selected={tab === 'queue'}
					onclick={() => (tab = 'queue')}>Queue</button
				>
				<button
					type="button"
					role="tab"
					aria-selected={tab === 'inbox'}
					onclick={() => (tab = 'inbox')}
					>Inbox {#if events.unread > 0}<b class="badge num">{events.unread}</b>{/if}</button
				>
				<button class="btn ghost sm close" type="button" onclick={() => (sheet = false)}>✕</button>
			</div>
			<div class="r-content">
				{#if tab === 'queue'}
					<QueueMini queue={events.queue} graph waitingList />
				{:else}
					<InboxList {api} store={events} />
				{/if}
			</div>
		</aside>
	</div>
	<button class="activity small" type="button" onclick={() => (sheet = true)}>
		<span class="mono num">{formatRate(events.queue.rate)}</span>
		<span>{events.queue.active.length}↓</span>
		{#if events.unread > 0}<b class="badge num">{events.unread}</b>{/if}
	</button>
</div>

<style>
	/* Phones first: TopNav's tab bar stays; the rail is a bottom sheet behind an Activity pill. */
	.r-app {
		min-height: 100%;
		padding-bottom: calc(var(--bottom-nav-h) + var(--safe-bottom) + 100px);
	}
	.r-rail {
		display: none;
	}
	.r-rail.sheet {
		display: flex;
		position: fixed;
		left: 0;
		right: 0;
		bottom: 0;
		max-height: 75vh;
		z-index: 60;
		border-radius: var(--r-xl) var(--r-xl) 0 0;
		box-shadow: var(--shadow);
	}
	.r-rail {
		flex-direction: column;
		background: var(--surface);
		border-left: 1px solid var(--line);
	}
	.tabs {
		display: flex;
		align-items: center;
		gap: var(--sp-1);
		padding: var(--sp-2) var(--sp-3);
		border-bottom: 1px solid var(--line);
	}
	.tabs [role='tab'] {
		border: 0;
		background: transparent;
		padding: 5px 10px;
		border-radius: var(--r-pill);
		font-weight: 500;
	}
	.tabs [aria-selected='true'] {
		background: var(--fg);
		color: var(--bg);
	}
	.close {
		margin-left: auto;
	}
	.r-content {
		padding: var(--sp-3);
		overflow: auto;
	}
	.badge {
		background: var(--warn);
		color: var(--surface);
		border-radius: var(--r-pill);
		font-size: var(--fs-xs);
		padding: 1px 6px;
	}
	.activity {
		position: fixed;
		right: var(--sp-3);
		bottom: calc(var(--bottom-nav-h) + var(--safe-bottom) + var(--sp-3));
		z-index: 25;
		display: flex;
		gap: var(--sp-2);
		align-items: center;
		padding: 7px 14px;
		border: 1px solid var(--line);
		border-radius: var(--r-pill);
		background: var(--surface);
		box-shadow: var(--shadow);
	}

	@media (min-width: 861px) {
		.r-app {
			padding-bottom: 60px;
		}
		.r-body {
			display: flex;
			align-items: flex-start;
		}
		main {
			flex: 1;
			min-width: 0;
		}
		.r-rail,
		.r-rail.sheet {
			display: flex;
			position: sticky;
			top: 53px;
			height: calc(100vh - 53px);
			max-height: none;
			width: 320px;
			flex: none;
			border-radius: 0;
			box-shadow: none;
		}
		.close,
		.activity {
			display: none;
		}
	}
</style>
