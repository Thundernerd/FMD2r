<script lang="ts">
	import MangaBakaProgress from '#lib/components/MangaBakaProgress.svelte';
	import { MangaBakaDatabase } from '#lib/mangabaka.svelte.ts';
	import type { StepProps } from '#lib/setup/steps.ts';

	let { api, store }: StepProps = $props();

	// The wizard gives a step its api and store once, for its lifetime.
	// svelte-ignore state_referenced_locally
	const mangabaka = new MangaBakaDatabase(api, store);
	let skipped = $state(false);

	/** Next waits for a choice, unless there is nothing left to choose. */
	export function ready(): boolean {
		return (
			skipped ||
			mangabaka.unavailable !== null ||
			mangabaka.running ||
			mangabaka.status?.downloaded === true
		);
	}
</script>

<p>
	A local copy of <a href="https://mangabaka.org" target="_blank" rel="noreferrer">MangaBaka</a>’s
	database gives Discover covers, format and status filters, and richer series details.
</p>
<p>
	The download is about 390 MB. No title is sent to MangaBaka: the titles are matched on this
	server, and only the download reaches MangaBaka.
</p>
<p class="small muted">You can add or remove it later in Settings → MangaBaka database.</p>

{#if mangabaka.unavailable !== null}
	<p class="bad small" role="alert">
		This server can’t download the MangaBaka database: {mangabaka.unavailable}. Discover works
		without it.
	</p>
{:else if mangabaka.running}
	<MangaBakaProgress download={mangabaka} />
	<p class="small muted">It downloads in the background; you can go on.</p>
{:else if mangabaka.status?.downloaded}
	<p>It is downloaded already.</p>
{:else}
	<div class="actions">
		<button
			class="btn primary"
			type="button"
			disabled={mangabaka.busy || !mangabaka.status}
			onclick={mangabaka.download}>Download now</button
		>
		<button
			class="btn"
			type="button"
			aria-pressed={skipped}
			disabled={mangabaka.busy}
			onclick={() => (skipped = true)}>Skip</button
		>
	</div>
	{#if skipped}
		<p class="small muted">Skipped. Discover works without it.</p>
	{/if}
{/if}
{#if mangabaka.error}
	<p class="bad small" role="alert">{mangabaka.error}</p>
{/if}

<style>
	p {
		margin: 0;
	}
	.actions {
		display: flex;
		gap: var(--sp-2);
	}
	.bad {
		color: var(--bad);
	}
</style>
