<script lang="ts">
	import type { Health } from '#lib/api/types.ts';
	import { isOpenServer } from '#lib/session.svelte.ts';

	/** Where the dismissal is kept: a per-browser convenience. */
	const DISMISSED_KEY = 'fmd2r.open-server-banner-dismissed';

	let { health }: { health: Health | null } = $props();

	function wasDismissed(): boolean {
		try {
			return localStorage.getItem(DISMISSED_KEY) === '1';
		} catch {
			return false;
		}
	}

	let dismissed = $state(wasDismissed());

	function dismiss() {
		dismissed = true;
		try {
			localStorage.setItem(DISMISSED_KEY, '1');
		} catch {
			// Shown again next time; nothing else depends on it.
		}
	}

	const open = $derived(health !== null && isOpenServer(health));

	// A dismissal covers one stretch of being open: once the server is secured (or bound to
	// loopback), forget it, so opening the server again brings the warning back.
	$effect(() => {
		if (health === null || open) return;
		dismissed = false;
		try {
			localStorage.removeItem(DISMISSED_KEY);
		} catch {
			// Nothing to forget, or nowhere to forget it from.
		}
	});
</script>

{#if open && !dismissed}
	<div class="open-server" role="alert">
		<span class="text">
			This server is reachable from other machines and no password is set: anyone who can reach it
			can use it.
		</span>
		<a class="link" href="/settings#section-server">Set a password</a>
		<button class="btn ghost sm" type="button" aria-label="Dismiss" onclick={dismiss}>✕</button>
	</div>
{/if}

<style>
	.open-server {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--sp-2) var(--sp-3);
		padding: var(--sp-2) var(--sp-4);
		background: var(--warn-soft);
		color: var(--fg);
		border-bottom: 1px solid var(--warn);
	}
	.text {
		flex: 1 1 240px;
	}
	.link {
		color: var(--warn);
		font-weight: 600;
	}
</style>
