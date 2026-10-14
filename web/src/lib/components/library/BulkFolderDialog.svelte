<script lang="ts">
	import { onMount } from 'svelte';
	import { ApiError, type Api } from '#lib/api/client.ts';
	import type { BulkChange, Destination } from '#lib/api/types.ts';
	import { defaultDestination, destinationLabel } from '#lib/destinations/destinations.ts';

	let {
		api,
		count,
		onpick,
		onclose
	}: {
		api: Api;
		/** How many series change folder. */
		count: number;
		/**
		 * Gives the series a folder in `destination`, with `move_files` also moving their chapters
		 * already downloaded; rejects when that fails.
		 */
		onpick: (change: Pick<BulkChange, 'destination' | 'move_files'>) => Promise<void>;
		onclose: () => void;
	} = $props();

	const uid = $props.id();
	let dialog: HTMLDialogElement | undefined = $state();
	let destinations = $state<Destination[]>([]);
	/** A destination's index, or `custom`. */
	let picked = $state('custom');
	let custom = $state('');
	/** Whether the chapters already downloaded move along. */
	let moveFiles = $state(false);
	let busy = $state(false);
	let error = $state<string | null>(null);
	let input: HTMLInputElement | undefined = $state();

	const folder = $derived(
		picked === 'custom' ? custom.trim() : (destinations[Number(picked)]?.path ?? '')
	);

	onMount(() => {
		dialog?.showModal();
		api
			.getSettings()
			.then((settings) => {
				destinations = settings.saveto.destinations;
				const preset = defaultDestination(destinations);
				if (preset) picked = String(destinations.indexOf(preset));
			})
			.catch(() => {});
	});

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (!folder) {
			error = 'Enter a folder.';
			input?.focus();
			return;
		}
		busy = true;
		error = null;
		try {
			await onpick(moveFiles ? { destination: folder, move_files: true } : { destination: folder });
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
	<form onsubmit={save}>
		<p class="small muted">
			Where new chapters of the {count} selected series are downloaded. Each series gets its own folder
			in it, named as a new download's.
			{#if moveFiles}
				Chapters already downloaded move to the new folder in the background; the inbox reports the
				result.
			{:else}
				Chapters already downloaded stay where they are; they are not moved.
			{/if}
		</p>
		<label class="field">
			<span class="label">Destination</span>
			<select class="input" bind:value={picked}>
				{#each destinations as d, i (i)}
					<option value={String(i)}>{destinationLabel(d)}</option>
				{/each}
				<option value="custom">Custom folder</option>
			</select>
		</label>
		{#if picked === 'custom'}
			<label class="field">
				<span class="label">Parent folder</span>
				<input
					class="input mono"
					type="text"
					autocomplete="off"
					bind:this={input}
					bind:value={custom}
				/>
			</label>
		{:else}
			<p class="small muted">In <span class="mono">{folder}</span></p>
		{/if}
		<label class="check">
			<input type="checkbox" bind:checked={moveFiles} />
			Move chapters already downloaded to the new folder
		</label>
		{#if error}
			<p class="error small" role="alert">{error}</p>
		{/if}
		<div class="actions">
			<button class="btn primary" type="submit" disabled={busy}>
				{busy ? 'Saving…' : 'Save'}
			</button>
			<button class="btn" type="button" onclick={() => dialog?.close()}>Cancel</button>
		</div>
	</form>
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
	.dialog[open],
	form {
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
	.check {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
	}
	.actions {
		display: flex;
		gap: var(--sp-2);
	}
	.error {
		color: var(--bad);
	}
</style>
