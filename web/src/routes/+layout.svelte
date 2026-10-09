<script lang="ts">
	import '#lib/styles/app.css';
	import { page } from '$app/state';
	import type { Snippet } from 'svelte';
	import { api, events, session } from '#lib/app.ts';
	import AddByUrl from '#lib/components/AddByUrl.svelte';
	import InboxPopover from '#lib/components/InboxPopover.svelte';
	import LoginScreen from '#lib/components/LoginScreen.svelte';
	import OpenServerBanner from '#lib/components/OpenServerBanner.svelte';
	import QueueDock from '#lib/components/QueueDock.svelte';
	import TopNav from '#lib/components/TopNav.svelte';

	let { children }: { children: Snippet } = $props();

	$effect(() => {
		// Again after logging in: the password may have changed meanwhile.
		if (session.locked) return;
		session.checkHealth(api).catch(() => {});
	});

	$effect(() => {
		// Behind the login screen nothing can be fetched; logging in starts over.
		if (session.locked) return;
		events.start();
		// The queue also refetches on connect, but must show even when the stream can't connect.
		events.queue.resync();
		api
			.listInbox()
			.then((inbox) => events.seed({ inbox }))
			.catch(() => {});
		// One speed sample a second for the dock's and the Queue page's graphs.
		const sampling = setInterval(() => events.queue.sample(), 1000);
		return () => {
			clearInterval(sampling);
			events.stop();
		};
	});

	// The Queue page shows the full queue, so the dock would only repeat it; the Settings page
	// puts its save bar where the dock sits.
	const showDock = $derived(!['/queue', '/settings'].includes(page.url.pathname));

	async function logout() {
		await api.logout().catch(() => {});
		session.unauthorized();
	}
</script>

{#if session.locked}
	<LoginScreen {api} onlogin={() => session.loggedIn()} />
{:else}
	<div class="app" class:with-dock={showDock}>
		<OpenServerBanner health={session.health} />
		<TopNav>
			<AddByUrl {api} />
			<InboxPopover {api} store={events} />
			{#if session.required}
				<button class="btn ghost sm" type="button" onclick={logout}>Log out</button>
			{/if}
		</TopNav>
		<main>
			{@render children()}
		</main>
		{#if showDock}
			<QueueDock queue={events.queue} />
		{/if}
	</div>
{/if}

<style>
	/* Leave room for the bottom tab bar (phones) and the queue dock. */
	.app {
		min-height: 100%;
		padding-bottom: calc(var(--bottom-nav-h) + var(--safe-bottom) + var(--sp-4));
	}
	.app.with-dock {
		padding-bottom: calc(var(--bottom-nav-h) + var(--safe-bottom) + 90px);
	}
	@media (min-width: 861px) {
		.app {
			padding-bottom: var(--sp-6);
		}
		.app.with-dock {
			padding-bottom: 130px;
		}
	}
</style>
