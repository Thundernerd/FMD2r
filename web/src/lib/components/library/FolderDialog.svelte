<script lang="ts">
	import { onMount } from 'svelte';
	import type { Api } from '#lib/api/client.ts';
	import type { Destination, FavoriteView } from '#lib/api/types.ts';
	import { joinPath, lastComponent, samePath } from '#lib/destinations/destinations.ts';

	let {
		api,
		favorite,
		onsaved,
		onclose
	}: {
		api: Api;
		/** The library series whose download folder changes. */
		favorite: FavoriteView;
		onsaved: (favorite: FavoriteView) => void;
		onclose: () => void;
	} = $props();

	const uid = $props.id();
	let dialog: HTMLDialogElement | undefined = $state();
	let destinations = $state<Destination[]>([]);
	/** Whether downloads go in a folder per manga, which a destination then gets appended. */
	let mangaFolder = $state(true);
	// The dialog edits a copy; the series changes only when saved.
	// svelte-ignore state_referenced_locally
	let folder = $state(favorite.save_to);
	let busy = $state(false);
	let error = $state<string | null>(null);

	onMount(() => {
		dialog?.showModal();
		api
			.getSettings()
			.then((settings) => {
				destinations = settings.saveto.destinations;
				mangaFolder = settings.saveto.generate_manga_folder;
			})
			.catch(() => {});
	});

	/** The folder in destination `d`: under it in the series' own folder, as FMD2 moves several. */
	const inside = (d: Destination) =>
		mangaFolder ? joinPath(d.path, lastComponent(favorite.save_to)) : d.path;

	/** The destination the folder is in, or `custom` for a folder in none. */
	const picked = $derived.by(() => {
		const i = destinations.findIndex(
			(d) => samePath(inside(d), folder) || samePath(d.path, folder)
		);
		return i < 0 ? 'custom' : String(i);
	});

	function pick(value: string) {
		const d = destinations[Number(value)];
		if (d) folder = inside(d);
	}

	async function save() {
		if (!folder.trim()) {
			error = 'Enter a folder.';
			return;
		}
		busy = true;
		error = null;
		try {
			onsaved(await api.updateFavorite(favorite.id, { save_to: folder.trim() }));
			dialog?.close();
		} catch {
			error = 'Could not change the folder.';
		} finally {
			busy = false;
		}
	}
</script>

<dialog class="dialog" bind:this={dialog} aria-labelledby="{uid}-title" {onclose}>
	<header>
		<h2 id="{uid}-title">Download folder</h2>
		<button class="btn ghost sm" type="button" onclick={() => dialog?.close()}>Close</button>
	</header>
	<p class="small muted">
		Where new chapters of <b>{favorite.title}</b> are downloaded. Chapters already downloaded stay
		in
		<span class="mono">{favorite.save_to}</span>; they are not moved.
	</p>
	<label class="field">
		<span class="label">Destination</span>
		<select class="input" value={picked} onchange={(e) => pick(e.currentTarget.value)}>
			{#each destinations as d, i (i)}
				<option value={String(i)}>{d.name}{d.default ? ' (default)' : ''}</option>
			{/each}
			<option value="custom" disabled={picked !== 'custom'}>Custom folder</option>
		</select>
	</label>
	<label class="field">
		<span class="label">Folder</span>
		<input class="input mono" type="text" autocomplete="off" bind:value={folder} />
	</label>
	{#if error}
		<p class="error small" role="alert">{error}</p>
	{/if}
	<div class="actions">
		<button class="btn primary" type="button" disabled={busy} onclick={save}>
			{busy ? 'Saving…' : 'Save'}
		</button>
		<button class="btn" type="button" onclick={() => dialog?.close()}>Cancel</button>
	</div>
</dialog>

<style>
	.dialog {
		width: min(520px, calc(100vw - 32px));
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
	p {
		margin: 0;
	}
	.mono {
		word-break: break-all;
	}
	.field {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
	}
	.actions {
		display: flex;
		gap: var(--sp-2);
	}
	.error {
		color: var(--bad);
	}
</style>
