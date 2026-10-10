<script lang="ts">
	import { onMount } from 'svelte';
	import type { Api } from '#lib/api/client.ts';
	import type { JobState } from '#lib/api/types.ts';
	import ImportFlow from './ImportFlow.svelte';

	let {
		api,
		job,
		onimported,
		onclose
	}: {
		api: Api;
		/** The `import` job, for its progress. */
		job: JobState | undefined;
		/** An import (not a dry run) finished. */
		onimported: () => void;
		onclose: () => void;
	} = $props();

	let dialog: HTMLDialogElement | undefined = $state();

	onMount(() => dialog?.showModal());
</script>

<dialog class="dialog" bind:this={dialog} aria-labelledby="import-title" {onclose}>
	<header>
		<h2 id="import-title">Import from FMD2</h2>
		<button class="btn ghost sm" type="button" onclick={() => dialog?.close()}>Close</button>
	</header>
	<ImportFlow {api} {job} {onimported} />
</dialog>

<style>
	.dialog {
		width: min(640px, calc(100vw - 32px));
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
		background: rgba(5, 15, 18, 0.45);
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
