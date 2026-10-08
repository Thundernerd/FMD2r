<script lang="ts">
	import { goto } from '$app/navigation';
	import type { Api } from '#lib/api/client.ts';

	let { api }: { api: Api } = $props();

	let url = $state('');
	let busy = $state(false);
	let error = $state<string | null>(null);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		// People often paste without the scheme (`mangadex.org/title/…`).
		const typed = url.trim();
		const value = typed && !/^[a-z][a-z\d+.-]*:\/\//i.test(typed) ? `https://${typed}` : typed;
		if (!value || busy) return;
		busy = true;
		error = null;
		try {
			const series = await api.resolveUrl(value);
			if (!series) {
				error = 'No module handles this URL.';
				return;
			}
			url = '';
			const link = series.link.split('/').map(encodeURIComponent).join('/');
			await goto(`/series/${encodeURIComponent(series.module)}/${link}`);
		} catch {
			error = 'Could not reach FMD2r. Try again.';
		} finally {
			busy = false;
		}
	}
</script>

<form class="add" onsubmit={submit}>
	<input
		class="input"
		type="text"
		inputmode="url"
		placeholder="Paste a manga URL"
		aria-label="Manga URL"
		bind:value={url}
		oninput={() => (error = null)}
	/>
	<button class="btn primary" type="submit" disabled={busy}>Add</button>
	{#if error}
		<p class="error" role="alert">{error}</p>
	{/if}
</form>

<style>
	.add {
		position: relative;
		flex: 1;
		display: flex;
		gap: 6px;
		min-width: 0;
		max-width: 460px;
	}
	.add .input {
		flex: 1;
	}
	.error {
		position: absolute;
		top: calc(100% + 6px);
		left: 0;
		margin: 0;
		padding: 6px 10px;
		border-radius: var(--r);
		background: var(--bad-soft);
		color: var(--bad);
		font-size: var(--fs-sm);
		box-shadow: var(--shadow);
		z-index: 30;
	}
</style>
