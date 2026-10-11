<!-- PROTOTYPE (#224) variant C "Command": no nav bar, an omnibox, a "Today" home. -->
<script lang="ts">
	import { page } from '$app/state';
	import type { Snippet } from 'svelte';
	import type { Api } from '#lib/api/client.ts';
	import type { EventStore } from '#lib/events.svelte.ts';
	import { formatRate } from '#lib/queue-format.ts';
	import CommandHome from './CommandHome.svelte';
	import Omnibox from './Omnibox.svelte';

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

	// The Library page moves to /?library; / is the Today page.
	const home = $derived(page.url.pathname === '/' && !page.url.searchParams.has('library'));
	const section = $derived(
		page.url.pathname === '/'
			? home
				? 'Today'
				: 'Library'
			: page.url.pathname === '/series'
				? 'Series'
				: page.url.pathname.slice(1).replace(/^./, (c) => c.toUpperCase())
	);
</script>

<div class="c-app">
	<header class="c-top">
		<a class="brand" href="/">FMD2r</a>
		<span class="crumb muted small">/ {section}</span>
		<div class="omni-slot"><Omnibox {api} /></div>
		<div class="chips">
			<a class="chip small" href="/queue" title="Queue">
				<span class="mono num">{formatRate(events.queue.rate)}</span>
				<span class="muted">{events.queue.active.length}↓ {events.queue.waiting}…</span>
			</a>
			<a class="chip small" href="/" title="Needs you" class:warn={events.unread > 0}
				>{events.unread} need you</a
			>
			{#if logout}
				<button class="btn ghost sm" type="button" onclick={logout}>Log out</button>
			{/if}
		</div>
	</header>
	{@render banner()}
	<main>
		{#if home}
			<CommandHome {api} {events} />
		{:else}
			{@render children()}
		{/if}
	</main>
</div>

<style>
	/* Phones first: brand and status chips on top, the omnibox fixed at the bottom. */
	.c-app {
		min-height: 100%;
		padding-bottom: calc(110px + var(--safe-bottom));
	}
	.c-top {
		position: sticky;
		top: var(--safe-top);
		z-index: 20;
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		padding: 10px var(--sp-4);
		background: var(--bg);
		border-bottom: 1px solid var(--line);
	}
	.brand {
		font: 800 var(--fs-xl) var(--f-display);
		letter-spacing: -0.02em;
		color: var(--fg);
		text-decoration: none;
	}
	.crumb {
		display: none;
	}
	.omni-slot {
		position: fixed;
		left: 0;
		right: 0;
		bottom: 0;
		z-index: 30;
		display: flex;
		justify-content: center;
		padding: var(--sp-2) var(--sp-3) calc(var(--sp-2) + var(--safe-bottom));
		background: var(--surface);
		border-top: 1px solid var(--line);
	}
	.chips {
		margin-left: auto;
		display: flex;
		gap: var(--sp-2);
		align-items: center;
	}
	.chip {
		display: flex;
		gap: var(--sp-1);
		padding: 3px 10px;
		border-radius: var(--r-pill);
		border: 1px solid var(--line);
		background: var(--surface);
		color: var(--fg);
		text-decoration: none;
		white-space: nowrap;
	}
	.chip.warn {
		border-color: var(--warn);
		color: var(--warn);
	}

	@media (min-width: 861px) {
		.c-app {
			padding-bottom: 60px;
		}
		.c-top {
			padding: var(--sp-3) var(--sp-5);
			gap: var(--sp-4);
		}
		.crumb {
			display: inline;
		}
		.omni-slot {
			position: static;
			flex: 1;
			padding: 0;
			background: transparent;
			border: 0;
		}
		.chips {
			margin-left: 0;
		}
	}
</style>
