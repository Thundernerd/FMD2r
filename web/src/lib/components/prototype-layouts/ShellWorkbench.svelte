<!-- PROTOTYPE (#224) variant W "Workbench": icon rail, list + detail panes, queue bottom panel. -->
<script lang="ts">
	import { page } from '$app/state';
	import type { Snippet } from 'svelte';
	import type { Api } from '#lib/api/client.ts';
	import type { EventStore } from '#lib/events.svelte.ts';
	import { formatRate, percent } from '#lib/queue-format.ts';
	import AddByUrl from '#lib/components/AddByUrl.svelte';
	import InboxPopover from '#lib/components/InboxPopover.svelte';
	import FavoritesList from './FavoritesList.svelte';
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

	// Open on desktop, a one-line strip on phones.
	let panelOpen = $state(window.matchMedia('(min-width: 861px)').matches);
	const queue = $derived(events.queue);
	// Library and Series share the list pane; the Queue page is the panel, full size; Settings keeps its save bar.
	const listPane = $derived(['/', '/series'].includes(page.url.pathname));
	const showPanel = $derived(!['/queue', '/settings'].includes(page.url.pathname));
</script>

<div class="w-app">
	<nav class="w-rail" aria-label="Main">
		<a class="mark" href="/" aria-label="FMD2r">F</a>
		{#each NAV as link (link.href)}
			<a
				class="rail-link"
				href={link.href}
				title={link.label}
				aria-current={isCurrent(link.href) ? 'page' : undefined}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true"><path d={link.icon} /></svg>
				<span class="rl-label">{link.label}</span>
			</a>
		{/each}
		<div class="inbox"><InboxPopover {api} store={events} /></div>
	</nav>
	<div class="w-body">
		{#if listPane}
			<aside class="w-list"><FavoritesList {api} /></aside>
		{/if}
		<div class="w-main">
			<div class="w-tools">
				<AddByUrl {api} />
				{#if logout}
					<button class="btn ghost sm" type="button" onclick={logout}>Log out</button>
				{/if}
			</div>
			{@render banner()}
			<main>{@render children()}</main>
		</div>
	</div>
	{#if showPanel}
		<section class="w-panel" class:open={panelOpen} aria-label="Download queue">
			<button class="p-head small" type="button" onclick={() => (panelOpen = !panelOpen)}>
				<b>Queue</b>
				<span class="mono num">{formatRate(queue.rate)}</span>
				<span class="muted">{queue.active.length} active · {queue.waiting} waiting</span>
				<span class="muted toggle">{panelOpen ? '▾' : '▴'}</span>
			</button>
			{#if panelOpen}
				<div class="p-body">
					<table class="small">
						<thead>
							<tr><th>Title</th><th>Status</th><th>Chapter</th><th>Progress</th><th>Speed</th></tr>
						</thead>
						<tbody>
							{#each queue.tasks as t (t.id)}
								<tr>
									<td class="title">{t.title}</td>
									<td>{t.status}</td>
									<td>{t.chapters}</td>
									<td class="mono num">
										{t.chapters_done}/{t.chapter_count}
										<span class="bar"><i style:width="{percent(t.done, t.total)}%"></i></span>
									</td>
									<td class="mono num">{t.bytes_per_sec ? formatRate(t.bytes_per_sec) : ''}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</section>
	{/if}
</div>

<style>
	/* Phones first: the rail is a bottom tab bar; no list pane; the panel is a strip above it. */
	.w-app {
		min-height: 100%;
		padding-bottom: calc(var(--bottom-nav-h) + var(--safe-bottom) + 80px);
	}
	.w-rail {
		position: fixed;
		left: 0;
		right: 0;
		bottom: 0;
		z-index: 30;
		height: calc(var(--bottom-nav-h) + var(--safe-bottom));
		padding-bottom: var(--safe-bottom);
		display: flex;
		align-items: center;
		background: var(--surface);
		border-top: 1px solid var(--line);
	}
	.mark,
	.w-list {
		display: none;
	}
	.rail-link {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 2px;
		padding: 6px 2px;
		color: var(--muted);
		text-decoration: none;
		font-size: var(--fs-xs);
	}
	.rail-link[aria-current='page'] {
		color: var(--accent);
	}
	.rail-link svg {
		width: 22px;
		height: 22px;
		fill: none;
		stroke: currentColor;
		stroke-width: 1.8;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	/* On phones the inbox sits at the right end of the tools row. */
	.inbox {
		position: fixed;
		top: calc(var(--safe-top) + 10px);
		right: var(--sp-3);
		z-index: 31;
	}
	.w-tools {
		display: flex;
		gap: var(--sp-2);
		align-items: center;
		padding: 10px 90px 10px var(--sp-3);
		background: var(--surface);
		border-bottom: 1px solid var(--line);
	}
	.w-panel {
		position: fixed;
		left: 0;
		right: 0;
		bottom: calc(var(--bottom-nav-h) + var(--safe-bottom));
		z-index: 25;
		background: var(--surface);
		border-top: 1px solid var(--line);
		box-shadow: var(--shadow);
	}
	.p-head {
		width: 100%;
		display: flex;
		gap: var(--sp-3);
		align-items: baseline;
		padding: var(--sp-2) var(--sp-3);
		border: 0;
		background: var(--surface-2);
		text-align: left;
	}
	.toggle {
		margin-left: auto;
	}
	.p-body {
		max-height: 30vh;
		overflow: auto;
	}
	table {
		width: 100%;
		border-collapse: collapse;
	}
	th,
	td {
		text-align: left;
		padding: 4px var(--sp-3);
		border-bottom: 1px solid var(--line);
		white-space: nowrap;
	}
	th {
		color: var(--muted);
		font-weight: 500;
	}
	.title {
		max-width: 260px;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.bar {
		display: inline-block;
		vertical-align: middle;
		width: 60px;
		height: 4px;
		border-radius: var(--r-pill);
		background: var(--surface-2);
		overflow: hidden;
	}
	.bar i {
		display: block;
		height: 100%;
		background: var(--accent);
	}

	/* Desktop: an IDE-like frame. Every pane scrolls on its own. */
	@media (min-width: 861px) {
		.w-app {
			display: flex;
			flex-direction: column;
			height: 100vh;
			padding: 0 0 0 64px;
		}
		.w-rail {
			top: 0;
			right: auto;
			width: 64px;
			height: auto;
			flex-direction: column;
			gap: 2px;
			padding: var(--sp-3) 0;
			border-top: 0;
			border-right: 1px solid var(--line);
		}
		.mark {
			display: block;
			margin-bottom: var(--sp-3);
			font: 800 var(--fs-title) var(--f-display);
			color: var(--accent);
			text-decoration: none;
		}
		.rail-link {
			flex: none;
			width: 52px;
			padding: 8px 0;
			border-radius: var(--r);
		}
		.rail-link:hover {
			background: var(--surface-2);
		}
		.rail-link[aria-current='page'] {
			background: var(--accent-soft);
		}
		.inbox {
			position: static;
			margin-top: auto;
		}
		.inbox :global(.bell) {
			padding: 4px 8px;
			font-size: var(--fs-xs);
		}
		.inbox :global(.pop) {
			left: calc(100% + var(--sp-2));
			right: auto;
			top: auto;
			bottom: 0;
		}
		.w-body {
			flex: 1;
			min-height: 0;
			display: flex;
		}
		.w-list {
			display: block;
			flex: none;
			width: 280px;
			border-right: 1px solid var(--line);
			background: var(--surface);
		}
		.w-main {
			flex: 1;
			min-width: 0;
			overflow: auto;
		}
		.w-tools {
			position: sticky;
			top: 0;
			z-index: 5;
			padding: var(--sp-2) var(--sp-4);
		}
		.w-panel {
			position: static;
			flex: none;
			box-shadow: none;
		}
	}
</style>
