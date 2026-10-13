<script lang="ts">
	import type { Api } from '#lib/api/client.ts';
	import type { ComicServerTest, Settings } from '#lib/api/types.ts';
	import type { Draft } from '#lib/settings/draft.svelte.ts';

	let { api, draft }: { api: Pick<Api, 'testComicServer'>; draft: Draft<Settings> } = $props();

	let testing = $state(false);
	let result = $state<ComicServerTest | null>(null);
	let failure = $state<string | null>(null);

	const server = $derived(draft.value.comic_server);
	const ready = $derived(server.kind !== 'none' && server.url.trim() !== '');

	/** Tests the settings as edited: a typed API key, else the stored one. */
	async function test() {
		testing = true;
		result = null;
		failure = null;
		// The draft holds `api_key` only while a key is typed (see the `secret` control).
		const typed = (draft.get('comic_server.api_key') as string | undefined) ?? null;
		try {
			result = await api.testComicServer({
				kind: server.kind,
				url: server.url,
				api_key: typed,
				library_id: server.library_id ?? null
			});
		} catch (e) {
			failure = e instanceof Error ? e.message : String(e);
		} finally {
			testing = false;
		}
	}
</script>

<div class="panel">
	<div class="actions">
		<button class="btn" type="button" disabled={!ready || testing} onclick={test}>
			Test connection
		</button>
		{#if testing}<span class="small muted">Testing…</span>{/if}
	</div>
	{#if result?.ok}
		<p class="ok small" role="status">{result.message}</p>
		{#if result.libraries.length}
			<ul class="libraries small">
				{#each result.libraries as library (library.id)}
					<li><span>{library.name}</span> <span class="mono muted">{library.id}</span></li>
				{/each}
			</ul>
		{/if}
	{:else if result}
		<p class="bad small" role="alert">{result.message}</p>
	{:else if failure}
		<p class="bad small" role="alert">Could not test the connection: {failure}</p>
	{/if}
</div>

<style>
	.panel {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
		padding-top: var(--sp-3);
		border-top: 1px solid var(--line);
	}
	p {
		margin: 0;
	}
	.actions {
		display: flex;
		gap: var(--sp-2);
		align-items: baseline;
	}
	.libraries {
		margin: 0;
		padding-left: var(--sp-4);
	}
	.ok {
		color: var(--ok);
	}
	.bad {
		color: var(--bad);
	}
</style>
