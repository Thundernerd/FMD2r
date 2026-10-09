<script lang="ts">
	import type { MangaBakaStatus } from '#lib/api/types.ts';

	/** Where the dismissal is kept: a per-browser convenience. */
	const DISMISSED_KEY = 'fmd2r.discover.mangabaka-hint-dismissed';

	let { status }: { status: MangaBakaStatus | null } = $props();

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
</script>

{#if status?.available && !status.downloaded && !dismissed}
	<div class="hint" role="note">
		<p class="small">
			Get formats, publication status and descriptions for list titles from MangaBaka (~390 MB
			download).
			<a href="/settings#section-metadata">Set up the MangaBaka database</a>
		</p>
		<button class="btn ghost sm" type="button" aria-label="Dismiss" onclick={dismiss}>✕</button>
	</div>
{/if}

<style>
	.hint {
		display: flex;
		align-items: flex-start;
		gap: var(--sp-2);
		padding: var(--sp-2) var(--sp-3);
		border: 1px solid var(--line);
		border-radius: var(--r);
		background: var(--accent-soft);
	}
	p {
		flex: 1;
		margin: 0;
	}
</style>
