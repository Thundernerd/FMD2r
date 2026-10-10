<script lang="ts">
	import type { Health } from '#lib/api/types.ts';
	import { isOpenServer } from '#lib/session.svelte.ts';

	let { health }: { health: Health | null } = $props();

	const open = $derived(health !== null && isOpenServer(health));
</script>

{#if open}
	<div class="open-server" role="alert">
		<span class="text">
			This server is reachable from other machines and no password is set: anyone who can reach it
			can use it.
		</span>
		<a class="link" href="/settings#section-server">Set a password</a>
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
