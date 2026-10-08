<script lang="ts">
	import '#lib/styles/app.css';
	import { page } from '$app/state';
	import type { Snippet } from 'svelte';
	import { api, events } from '#lib/app.ts';
	import AddByUrl from '#lib/components/AddByUrl.svelte';
	import InboxPopover from '#lib/components/InboxPopover.svelte';
	import QueueDock from '#lib/components/QueueDock.svelte';
	import TopNav from '#lib/components/TopNav.svelte';

	let { children }: { children: Snippet } = $props();

	$effect(() => {
		events.start();
		// Snapshot what happened before the stream connected; frames that already arrived win.
		api
			.listInbox()
			.then((items) => {
				const seen = new Set(events.inbox.map((i) => i.id));
				events.inbox = [...events.inbox, ...items.filter((i) => !seen.has(i.id))];
			})
			.catch(() => {});
		api
			.listTasks()
			.then((tasks) => {
				for (const task of tasks) events.tasks[task.id] ??= task;
			})
			.catch(() => {});
		return () => events.stop();
	});

	// The Queue page shows the full queue, so the dock would only repeat it.
	const showDock = $derived(page.url.pathname !== '/queue');
</script>

<div class="app" class:with-dock={showDock}>
	<TopNav>
		<AddByUrl {api} />
		<InboxPopover {api} store={events} />
	</TopNav>
	<main>
		{@render children()}
	</main>
	{#if showDock}
		<QueueDock store={events} />
	{/if}
</div>

<style>
	.app {
		min-height: 100%;
		padding-bottom: var(--sp-6);
	}
	.app.with-dock {
		padding-bottom: 130px;
	}
	@media (max-width: 860px) {
		.app {
			padding-bottom: calc(var(--bottom-nav-h) + var(--safe-bottom) + var(--sp-4));
		}
		.app.with-dock {
			padding-bottom: calc(var(--bottom-nav-h) + var(--safe-bottom) + 90px);
		}
	}
</style>
