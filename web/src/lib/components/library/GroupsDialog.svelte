<script lang="ts">
	import { onMount } from 'svelte';
	import type { Api } from '#lib/api/client.ts';
	import type { GroupView } from '#lib/api/types.ts';
	import GroupEditor from './GroupEditor.svelte';

	let {
		api,
		counts,
		onchange,
		onclose
	}: {
		api: Api;
		/** Series per group id. */
		counts: Record<number, number>;
		onchange: (groups: GroupView[]) => void;
		onclose: () => void;
	} = $props();

	let dialog: HTMLDialogElement | undefined = $state();

	onMount(() => dialog?.showModal());
</script>

<dialog class="dialog" bind:this={dialog} aria-labelledby="groups-title" {onclose}>
	<header>
		<h2 id="groups-title">Groups</h2>
		<button class="btn ghost sm" type="button" onclick={() => dialog?.close()}>Close</button>
	</header>
	<GroupEditor {api} {counts} {onchange} />
</dialog>

<style>
	.dialog {
		width: min(520px, calc(100vw - 32px));
		max-height: calc(100vh - 48px);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		background: var(--surface);
		color: var(--fg);
		box-shadow: var(--shadow);
		padding: var(--sp-4);
	}
	.dialog[open] {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
	}
	.dialog::backdrop {
		background: var(--scrim);
	}
	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--sp-2);
	}
	h2 {
		margin: 0;
		font-size: var(--fs-lg);
	}
</style>
