<!-- PROTOTYPE (#224) variant S "Sidebar": nav, add-by-URL, inbox and queue in a left sidebar. -->
<script lang="ts">
	import { page } from '$app/state';
	import type { Snippet } from 'svelte';
	import type { Api } from '#lib/api/client.ts';
	import type { EventStore } from '#lib/events.svelte.ts';
	import { formatRate } from '#lib/queue-format.ts';
	import AddByUrl from '#lib/components/AddByUrl.svelte';
	import InboxPopover from '#lib/components/InboxPopover.svelte';
	import QueueMini from './QueueMini.svelte';
	import { NAV, isCurrent } from './variant.svelte.ts';

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

	let open = $state(false);
	// Navigating closes the phone drawer.
	$effect(() => {
		void page.url.pathname;
		open = false;
	});
</script>

<div class="s-app">
	<header class="s-bar">
		<button class="btn ghost sm" type="button" aria-label="Menu" onclick={() => (open = true)}
			>☰</button
		>
		<a class="brand" href="/">FMD2r</a>
		<a class="pill small mono" href="/queue"
			>{formatRate(events.queue.rate)} · {events.queue.active.length}↓</a
		>
	</header>
	{#if open}
		<button class="scrim" type="button" aria-label="Close menu" onclick={() => (open = false)}
		></button>
	{/if}
	<aside class="s-side" class:open>
		<a class="brand" href="/">FMD2r</a>
		<div class="add"><AddByUrl {api} /></div>
		<nav class="nav" aria-label="Main">
			{#each NAV as link (link.href)}
				<a
					class="nav-link"
					href={link.href}
					aria-current={isCurrent(link.href) ? 'page' : undefined}
				>
					<svg class="nav-icon" viewBox="0 0 24 24" aria-hidden="true"><path d={link.icon} /></svg>
					<span>{link.label}</span>
				</a>
			{/each}
		</nav>
		<div class="inbox"><InboxPopover {api} store={events} /></div>
		<section class="queue" aria-label="Download queue">
			<div class="label small muted">Queue</div>
			<QueueMini queue={events.queue} limit={3} graph />
		</section>
		{#if logout}
			<button class="btn ghost sm" type="button" onclick={logout}>Log out</button>
		{/if}
	</aside>
	<div class="s-main">
		{@render banner()}
		<main>{@render children()}</main>
	</div>
</div>

<style>
	/* Phones first: a slim top bar, the sidebar is a drawer. */
	.s-app {
		min-height: 100%;
		padding-bottom: 70px;
	}
	.s-bar {
		position: sticky;
		top: var(--safe-top);
		z-index: 20;
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		padding: var(--sp-2) var(--sp-3);
		background: var(--surface);
		border-bottom: 1px solid var(--line);
	}
	.brand {
		font: 800 var(--fs-xl) var(--f-display);
		letter-spacing: -0.02em;
		color: var(--fg);
		text-decoration: none;
	}
	.pill {
		margin-left: auto;
		padding: 3px 10px;
		border-radius: var(--r-pill);
		background: var(--accent-soft);
		color: var(--accent);
		text-decoration: none;
	}
	.scrim {
		position: fixed;
		top: 0;
		left: 0;
		right: 0;
		bottom: 0;
		z-index: 39;
		border: 0;
		background: var(--scrim);
	}
	.s-side {
		position: fixed;
		top: 0;
		bottom: 0;
		left: 0;
		z-index: 40;
		width: 270px;
		max-width: 85%;
		transform: translateX(-105%);
		transition: transform 0.2s;
		display: flex;
		flex-direction: column;
		gap: var(--sp-4);
		padding: var(--sp-4);
		background: var(--surface);
		border-right: 1px solid var(--line);
	}
	.s-side.open {
		transform: none;
	}
	.add :global(.add) {
		flex-wrap: wrap;
	}
	.nav {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.nav-link {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		padding: 7px 10px;
		border-radius: var(--r);
		color: var(--fg);
		text-decoration: none;
		font-weight: 500;
	}
	.nav-link:hover {
		background: var(--surface-2);
	}
	.nav-link[aria-current='page'] {
		background: var(--accent-soft);
		color: var(--accent);
		font-weight: 600;
	}
	.nav-icon {
		width: 18px;
		height: 18px;
		fill: none;
		stroke: currentColor;
		stroke-width: 1.8;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	/* The popover opens beside the sidebar, not off its left edge. */
	.inbox :global(.pop) {
		left: 0;
		right: auto;
	}
	.queue {
		margin-top: auto;
		padding-top: var(--sp-3);
		border-top: 1px solid var(--line);
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.label {
		text-transform: uppercase;
		letter-spacing: 0.06em;
	}

	@media (min-width: 861px) {
		.s-app {
			display: flex;
			padding-bottom: 0;
		}
		.s-bar,
		.scrim {
			display: none;
		}
		.s-side {
			position: sticky;
			height: 100vh;
			flex: none;
			width: 250px;
			transform: none;
			transition: none;
		}
		.inbox :global(.pop) {
			left: calc(100% + var(--sp-4));
			top: 0;
		}
		.s-main {
			flex: 1;
			min-width: 0;
			padding-bottom: 60px;
		}
	}
</style>
