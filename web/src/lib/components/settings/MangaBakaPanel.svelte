<script lang="ts">
	import type { Api } from '#lib/api/client.ts';
	import type { EventStore } from '#lib/events.svelte.ts';
	import { MangaBakaDatabase } from '#lib/mangabaka.svelte.ts';
	import MangaBakaProgress from '#lib/components/MangaBakaProgress.svelte';

	let { api, store }: { api: Api; store: EventStore } = $props();

	// Settings gives the panel its api and store once, for its lifetime.
	// svelte-ignore state_referenced_locally
	const mangabaka = new MangaBakaDatabase(api, store);

	const megabytes = (bytes: number) => `${Math.round(bytes / 1_000_000).toLocaleString('en')} MB`;
	const date = (iso: string) =>
		new Date(iso).toLocaleString('en', { dateStyle: 'medium', timeStyle: 'short' });
</script>

<div class="panel">
	<p class="small muted">
		A local copy of <a href="https://mangabaka.org" target="_blank" rel="noreferrer">MangaBaka</a>’s
		database gives list titles a format, a publication status and descriptions, matched offline: no
		title leaves this server. Nothing is downloaded until you ask; the download is about 390 MB.
	</p>
	{#if mangabaka.status}
		{@const status = mangabaka.status}
		{#if status.downloaded && status.built_at}
			<p class="state">
				<span>Built {date(status.built_at)}</span>
				{#if status.bytes != null}<span class="muted">· {megabytes(status.bytes)}</span>{/if}
				{#if status.next_refresh}
					<span class="muted small">· next update {date(status.next_refresh)}</span>
				{/if}
			</p>
		{:else}
			<p class="state muted">Not downloaded.</p>
		{/if}

		{#if mangabaka.running}
			<MangaBakaProgress download={mangabaka} />
		{/if}

		<div class="actions">
			{#if mangabaka.running}
				<button class="btn" type="button" disabled={mangabaka.busy} onclick={mangabaka.cancel}
					>Cancel</button
				>
			{:else if status.downloaded}
				<button
					class="btn"
					type="button"
					disabled={mangabaka.busy || !status.available}
					onclick={mangabaka.download}>Update</button
				>
				<button class="btn" type="button" disabled={mangabaka.busy} onclick={mangabaka.remove}
					>Remove</button
				>
			{:else}
				<button
					class="btn primary"
					type="button"
					disabled={mangabaka.busy || !status.available}
					onclick={mangabaka.download}>Download</button
				>
			{/if}
		</div>
		{#if mangabaka.unavailable}
			<p class="small muted">This server cannot download it: {mangabaka.unavailable}.</p>
		{/if}
	{/if}
	{#if mangabaka.error}
		<p class="bad small" role="alert">{mangabaka.error}</p>
	{/if}
</div>

<style>
	.panel {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
		padding-top: var(--sp-3);
		border-top: 1px solid var(--line);
	}
	p {
		margin: 0;
	}
	.state {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-1);
		align-items: baseline;
	}
	.actions {
		display: flex;
		gap: var(--sp-2);
	}
	.bad {
		color: var(--bad);
	}
</style>
