<script lang="ts">
	import { onMount } from 'svelte';
	import { ApiError, type Api } from '#lib/api/client.ts';
	import type { Destination, FavoriteView } from '#lib/api/types.ts';
	import { destinationLabel, samePath } from '#lib/destinations/destinations.ts';

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
	/** The folder new chapters go to in each destination, as the server makes it, by index. */
	let folders = $state<string[]>([]);
	// The dialog edits a copy; the series changes only when saved.
	// svelte-ignore state_referenced_locally
	let folder = $state(favorite.save_to);
	/** Whether "Custom folder" is picked, so typing a destination's folder keeps it picked. */
	let custom = $state(false);
	/** Whether the chapters already downloaded move along. */
	let moveFiles = $state(false);
	let busy = $state(false);
	let error = $state<string | null>(null);
	let input: HTMLInputElement | undefined = $state();

	onMount(() => {
		dialog?.showModal();
		api
			.getSettings()
			.then(async (settings) => {
				const list = settings.saveto.destinations;
				// The manga folder as a new download makes it (`save_to`, frmMain.pas:2685-2710).
				folders = await Promise.all(
					list.map((d) =>
						api
							.saveFolder({ module_id: favorite.module_id, title: favorite.title, save_to: d.path })
							.catch(() => d.path)
					)
				);
				destinations = list;
			})
			.catch(() => {});
	});

	/** The destination the folder is in, or `custom` for a folder in none. */
	const picked = $derived.by(() => {
		if (custom) return 'custom';
		const i = folders.findIndex((f) => samePath(f, folder));
		return i < 0 ? 'custom' : String(i);
	});

	function pick(value: string) {
		custom = value === 'custom';
		const next = folders[Number(value)];
		if (custom) input?.focus();
		else if (next !== undefined) folder = next;
	}

	async function save() {
		if (!folder.trim()) {
			error = 'Enter a folder.';
			return;
		}
		busy = true;
		error = null;
		try {
			const save_to = folder.trim();
			onsaved(
				await api.updateFavorite(
					favorite.id,
					moveFiles ? { save_to, move_files: true } : { save_to }
				)
			);
			dialog?.close();
		} catch (e) {
			error =
				e instanceof ApiError && e.status === 409 && e.detail
					? e.detail
					: 'Could not change the folder.';
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
		Where new chapters of <b>{favorite.title}</b> are downloaded.
		{#if moveFiles}
			Chapters already downloaded in <span class="mono">{favorite.save_to}</span> move to the new folder
			in the background; the inbox reports the result.
		{:else}
			Chapters already downloaded stay in
			<span class="mono">{favorite.save_to}</span>; they are not moved.
		{/if}
	</p>
	<label class="field">
		<span class="label">Destination</span>
		<select class="input" value={picked} onchange={(e) => pick(e.currentTarget.value)}>
			{#each destinations as d, i (i)}
				<option value={String(i)}>{destinationLabel(d)}</option>
			{/each}
			<option value="custom">Custom folder</option>
		</select>
	</label>
	<label class="field">
		<span class="label">Folder</span>
		<input
			class="input mono"
			type="text"
			autocomplete="off"
			bind:this={input}
			bind:value={folder}
		/>
	</label>
	<label class="check">
		<input type="checkbox" bind:checked={moveFiles} />
		Move chapters already downloaded to the new folder
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
	.check {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
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
