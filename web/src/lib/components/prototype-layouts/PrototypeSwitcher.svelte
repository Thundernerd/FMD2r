<!--
	PROTOTYPE (#224), throwaway, never merge to main.

	Question: which app layout should FMD2r use by default? Five app shells, switchable with
	`?variant=` (kept while you navigate) or this bar / the ← → keys, all on the real routes and the
	mock backend (`npm run dev:mock`):

	  B  Library (today)  top bar nav + add-by-URL + inbox popover, Library grid home, queue dock.
	  S  Sidebar          left sidebar with nav, add-by-URL, inbox and a live queue summary; no dock.
	                      Phones: a slim top bar with a ☰ drawer and a queue speed pill.
	  W  Workbench        icon rail + favorites list pane + detail pane (Library/Series side by side),
	                      the queue as a collapsible bottom panel. Phones: rail → tab bar, no list pane.
	  C  Command          no nav bar: an omnibox (URL / pages / favorites / Discover search, `/` or
	                      Ctrl+K), a "Today" home (inbox, new chapters, downloads); Library at
	                      /?library. Phones: the omnibox sits at the bottom, by the thumb.
	  R  Queue rail       B's top bar, but queue and inbox live in an always-open right rail.
	                      Phones: tab bar stays; the rail is a bottom sheet behind an Activity pill.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { VARIANTS, currentVariant, picked, type VariantKey } from './variant.svelte.ts';

	/** How far above the bottom edge to sit, on phones and from 861px, so chrome stays clear. */
	let { liftPhone, liftDesktop }: { liftPhone: string; liftDesktop: string } = $props();

	const key = $derived(currentVariant());
	const index = $derived(VARIANTS.findIndex((v) => v.key === key));
	const current = $derived(VARIANTS[index] ?? VARIANTS[0]);

	function go(step: number) {
		const next: VariantKey =
			VARIANTS[(index + step + VARIANTS.length) % VARIANTS.length]?.key ?? 'B';
		picked.key = next;
		const url = new URL(page.url.href);
		url.searchParams.set('variant', next);
		goto(url, { replace: true, reset: false });
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
		const t = event.target;
		if (
			t instanceof HTMLInputElement ||
			t instanceof HTMLTextAreaElement ||
			t instanceof HTMLSelectElement ||
			(t instanceof HTMLElement && t.isContentEditable)
		) {
			return;
		}
		go(event.key === 'ArrowRight' ? 1 : -1);
	}
</script>

<svelte:window onkeydown={onKeydown} />

<div
	class="pswitch"
	role="toolbar"
	aria-label="Prototype layout switcher"
	style:--lift-phone={liftPhone}
	style:--lift-desktop={liftDesktop}
>
	<button type="button" aria-label="Previous layout" onclick={() => go(-1)}>←</button>
	<span class="plabel"><b>{current.key}</b> {current.name}</span>
	<button type="button" aria-label="Next layout" onclick={() => go(1)}>→</button>
</div>

<style>
	/* Inverted pill so it can't be mistaken for part of a layout. */
	.pswitch {
		position: fixed;
		left: 50%;
		transform: translateX(-50%);
		bottom: calc(var(--lift-phone) + var(--safe-bottom));
		z-index: 100;
		display: flex;
		align-items: center;
		gap: var(--sp-1);
		padding: 3px;
		border-radius: var(--r-pill);
		background: var(--fg);
		color: var(--bg);
		box-shadow: var(--shadow);
		font-size: var(--fs-sm);
		white-space: nowrap;
	}
	.pswitch button {
		border: 0;
		background: transparent;
		color: inherit;
		padding: 2px 10px;
		border-radius: var(--r-pill);
	}
	.pswitch button:hover {
		background: var(--muted);
	}
	.plabel {
		padding: 0 var(--sp-1);
	}
	@media (min-width: 861px) {
		.pswitch {
			bottom: calc(var(--lift-desktop) + var(--safe-bottom));
		}
	}
</style>
