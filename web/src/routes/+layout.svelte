<script lang="ts">
	import '#lib/styles/app.css';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import type { Snippet } from 'svelte';
	import { api, events, session } from '#lib/app.ts';
	import { adoptAppearance } from '#lib/appearance.ts';
	import AddByUrl from '#lib/components/AddByUrl.svelte';
	import InboxPopover from '#lib/components/InboxPopover.svelte';
	import LoginScreen from '#lib/components/LoginScreen.svelte';
	import OpenServerBanner from '#lib/components/OpenServerBanner.svelte';
	import QueueDock from '#lib/components/QueueDock.svelte';
	import TopNav from '#lib/components/TopNav.svelte';
	// PROTOTYPE (#224): alternative app shells behind `?variant=`; see PrototypeSwitcher.svelte.
	import PrototypeSwitcher from '#lib/components/prototype-layouts/PrototypeSwitcher.svelte';
	import ShellCommand from '#lib/components/prototype-layouts/ShellCommand.svelte';
	import ShellRail from '#lib/components/prototype-layouts/ShellRail.svelte';
	import ShellSidebar from '#lib/components/prototype-layouts/ShellSidebar.svelte';
	import ShellWorkbench from '#lib/components/prototype-layouts/ShellWorkbench.svelte';
	import {
		currentVariant,
		picked,
		prototypesOn,
		urlHasVariant,
		type VariantKey
	} from '#lib/components/prototype-layouts/variant.svelte.ts';
	import { setup } from '#lib/setup/status.svelte.ts';

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

	$effect(() => {
		// Behind the login screen the settings can't be read; logging in asks again.
		if (session.locked) return;
		// app.html applied the cached appearance; this corrects it if it changed on another device.
		setup.check(api).then((settings) => {
			if (settings) adoptAppearance(settings.appearance);
		});
	});

	const onSetup = $derived(page.url.pathname === '/setup');
	// Until setup is finished every page leads to it; run again from Settings, it doesn't.
	const redirecting = $derived(setup.completed === false && !onSetup);
	$effect(() => {
		if (redirecting) goto('/setup', { replaceState: true });
	});

	// The Queue page shows the full queue, so the dock would only repeat it; the Settings page
	// puts its save bar where the dock sits.
	const showDock = $derived(!['/queue', '/settings'].includes(page.url.pathname));

	// PROTOTYPE (#224): remember the variant, and put it back on links that drop it.
	const variant = $derived(prototypesOn ? currentVariant() : 'B');
	$effect(() => {
		if (!prototypesOn) return;
		const url = page.url;
		if (urlHasVariant(url)) {
			picked.key = currentVariant();
		} else if (picked.key !== 'B') {
			const next = new URL(url.href);
			next.searchParams.set('variant', picked.key);
			goto(next, { replace: true, reset: false });
		}
	});
	// Where the switcher sits so it clears each shell's bottom chrome (phone, desktop).
	const LIFT: Record<VariantKey, [string, string]> = {
		B: ['calc(var(--bottom-nav-h) + 76px)', '96px'],
		S: ['16px', '16px'],
		W: ['calc(var(--bottom-nav-h) + 48px)', '44px'],
		C: ['72px', '16px'],
		R: ['calc(var(--bottom-nav-h) + 70px)', '16px']
	};

	async function logout() {
		await api.logout().catch(() => {});
		session.unauthorized();
	}
</script>

{#if session.locked}
	<LoginScreen {api} onlogin={() => session.loggedIn()} />
{:else if setup.completed === null || redirecting}
	<!-- Nothing until it is known whether setup comes first. -->
{:else if onSetup}
	<main>
		{@render children()}
	</main>
{:else if variant === 'B'}
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
{:else}
	{@const shellProps = { api, events, logout: session.required ? logout : null }}
	{#snippet banner()}<OpenServerBanner health={session.health} />{/snippet}
	{#if variant === 'S'}
		<ShellSidebar {...shellProps} {banner}>{@render children()}</ShellSidebar>
	{:else if variant === 'W'}
		<ShellWorkbench {...shellProps} {banner}>{@render children()}</ShellWorkbench>
	{:else if variant === 'C'}
		<ShellCommand {...shellProps} {banner}>{@render children()}</ShellCommand>
	{:else}
		<ShellRail {...shellProps} {banner}>{@render children()}</ShellRail>
	{/if}
{/if}

{#if prototypesOn && !session.locked && !onSetup}
	<PrototypeSwitcher liftPhone={LIFT[variant][0]} liftDesktop={LIFT[variant][1]} />
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
