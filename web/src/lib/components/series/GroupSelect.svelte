<script lang="ts">
	import { ApiError, type Api } from '#lib/api/client.ts';
	import type { FavoriteView, GroupView } from '#lib/api/types.ts';

	let {
		api,
		favorite,
		onchange
	}: {
		api: Api;
		/** This series in the library, as the header loaded it. */
		favorite: FavoriteView;
		/** The series moved to another group (or none); passes its entry as the server saved it. */
		onchange: (favorite: FavoriteView) => void;
	} = $props();

	/** The `<select>` value that opens the new-group form. */
	const NEW = 'new';

	/** `null` until loaded, or when they could not be. */
	let groups = $state<GroupView[] | null>(null);
	let creating = $state(false);
	let newName = $state('');
	let busy = $state(false);
	let error = $state<string | null>(null);

	/** How the select shows a group id: empty for none. */
	const shown = (groupId: number | null | undefined) => (groupId == null ? '' : String(groupId));

	async function loadGroups() {
		groups = await api.listGroups();
	}
	$effect(() => {
		// Any failure, thrown or rejected, leaves the select out and says so.
		loadGroups().catch(() => (error = 'Could not load the groups.'));
	});

	async function assign(groupId: number | null) {
		onchange(await api.updateFavorite(favorite.id, { group_id: groupId }));
	}

	async function changed(select: HTMLSelectElement) {
		error = null;
		if (select.value === NEW) {
			creating = true;
			return;
		}
		creating = false;
		try {
			await assign(select.value ? Number(select.value) : null);
		} catch {
			error = 'Could not change the group.';
			select.value = shown(favorite.group_id);
		}
	}

	async function create(event: SubmitEvent) {
		event.preventDefault();
		const name = newName.trim();
		if (!name) return;
		busy = true;
		error = null;
		let group: GroupView;
		try {
			group = await api.createGroup(name);
			groups = [...(groups ?? []), group];
			creating = false;
			newName = '';
		} catch (e) {
			error =
				e instanceof ApiError && e.status === 409
					? `A group named ${name} exists already.`
					: 'Could not create the group.';
			busy = false;
			return;
		}
		try {
			await assign(group.id);
		} catch {
			// The group exists now: picking it from the select retries the move.
			error = 'Could not change the group.';
		} finally {
			busy = false;
		}
	}

	function cancel() {
		creating = false;
		newName = '';
		error = null;
	}
</script>

{#if groups}
	<select
		class="input group"
		aria-label="Group"
		value={creating ? NEW : shown(favorite.group_id)}
		onchange={(e) => changed(e.currentTarget)}
	>
		<option value="">No group</option>
		{#each groups as group (group.id)}
			<option value={String(group.id)}>{group.name}</option>
		{/each}
		<option value={NEW}>New group…</option>
	</select>
	{#if creating}
		<form class="new" onsubmit={create}>
			<input
				class="input"
				aria-label="New group name"
				placeholder="Group name"
				bind:value={newName}
			/>
			<button class="btn sm" type="submit" disabled={busy || !newName.trim()}>Create</button>
			<button class="btn sm ghost" type="button" onclick={cancel}>Cancel</button>
		</form>
	{/if}
{/if}
{#if error}
	<span class="small bad" role="alert">{error}</span>
{/if}

<style>
	.group {
		width: auto;
	}
	.new {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
	}
	.bad {
		color: var(--bad);
	}
</style>
