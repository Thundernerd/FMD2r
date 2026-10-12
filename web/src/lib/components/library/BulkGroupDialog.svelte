<script lang="ts">
	import { onMount } from 'svelte';
	import { ApiError, type Api } from '#lib/api/client.ts';
	import type { GroupView } from '#lib/api/types.ts';

	let {
		api,
		count,
		onpick,
		onclose
	}: {
		api: Api;
		/** How many series move. */
		count: number;
		/** Moves the series to the group (`null`: no group); rejects when that fails. */
		onpick: (groupId: number | null) => Promise<void>;
		onclose: () => void;
	} = $props();

	/** The `<select>` value that asks for a new group's name. */
	const NEW = 'new';

	const uid = $props.id();
	let dialog: HTMLDialogElement | undefined = $state();
	let groups = $state<GroupView[]>([]);
	/** A group id, empty for no group, or {@link NEW}. */
	let picked = $state('');
	let newName = $state('');
	let busy = $state(false);
	let error = $state<string | null>(null);

	onMount(() => {
		dialog?.showModal();
		api
			.listGroups()
			.then((list) => (groups = list))
			.catch(() => (error = 'Could not load the groups.'));
	});

	async function save(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = null;
		let groupId = picked ? Number(picked) : null;
		if (picked === NEW) {
			const name = newName.trim();
			try {
				const group = await api.createGroup(name);
				groups = [...groups, group];
				// The group exists now: saving again picks it instead of creating another.
				picked = String(group.id);
				groupId = group.id;
			} catch (e) {
				error =
					e instanceof ApiError && e.status === 409
						? `A group named ${name} exists already.`
						: 'Could not create the group.';
				busy = false;
				return;
			}
		}
		try {
			await onpick(groupId);
			dialog?.close();
		} catch {
			error = 'Could not change the group.';
		} finally {
			busy = false;
		}
	}
</script>

<dialog class="dialog" bind:this={dialog} aria-labelledby="{uid}-title" {onclose}>
	<header>
		<h2 id="{uid}-title">Group</h2>
		<button class="btn ghost sm" type="button" onclick={() => dialog?.close()}>Close</button>
	</header>
	<form onsubmit={save}>
		<p class="small muted">The group the {count} selected series go in.</p>
		<label class="field">
			<span class="label">Group</span>
			<select class="input" bind:value={picked}>
				<option value="">No group</option>
				{#each groups as group (group.id)}
					<option value={String(group.id)}>{group.name}</option>
				{/each}
				<option value={NEW}>New group…</option>
			</select>
		</label>
		{#if picked === NEW}
			<label class="field">
				<span class="label">New group name</span>
				<input class="input" placeholder="Group name" bind:value={newName} />
			</label>
		{/if}
		{#if error}
			<p class="error small" role="alert">{error}</p>
		{/if}
		<div class="actions">
			<button
				class="btn primary"
				type="submit"
				disabled={busy || (picked === NEW && !newName.trim())}
			>
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
