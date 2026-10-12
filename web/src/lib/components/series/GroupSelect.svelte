<script lang="ts">
	import { ApiError, type Api } from '#lib/api/client.ts';
	import type { GroupView } from '#lib/api/types.ts';

	let { api, module, link }: { api: Api; module: string; link: string } = $props();

	/** The `<select>` value that opens the new-group form. */
	const NEW = 'new';

	let favoriteId = $state<number | null>(null);
	let groups = $state<GroupView[]>([]);
	/** The group id as the select shows it; empty for none. */
	let value = $state('');
	let creating = $state(false);
	let newName = $state('');
	let busy = $state(false);
	let error = $state<string | null>(null);

	/** How the select shows a group id: empty for none. */
	const shown = (groupId: number | null | undefined) => (groupId == null ? '' : String(groupId));

	/** The series' entry in the library and the groups to choose from. */
	async function load() {
		const [favorites, list] = await Promise.all([api.listFavorites(), api.listGroups()]);
		const favorite = favorites.find((f) => f.module_id === module && f.link === link);
		if (!favorite) return;
		favoriteId = favorite.id;
		groups = list;
		value = shown(favorite.group_id);
	}
	$effect(() => {
		// Without the select, the rest of the header still works.
		load().catch(() => {});
	});

	async function assign(groupId: number | null) {
		if (favoriteId === null) return;
		const favorite = await api.updateFavorite(favoriteId, { group_id: groupId });
		value = shown(favorite.group_id);
	}

	async function changed() {
		error = null;
		if (value === NEW) {
			creating = true;
			return;
		}
		creating = false;
		try {
			await assign(value ? Number(value) : null);
		} catch {
			error = 'Could not change the group.';
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
			groups = [...groups, group];
			creating = false;
			newName = '';
			// The group exists now: picking it again from the select retries a failed move.
			value = shown(group.id);
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
			error = 'Could not change the group.';
		} finally {
			busy = false;
		}
	}

	function cancel() {
		creating = false;
		newName = '';
		error = null;
		const current = groups.find((g) => String(g.id) === value);
		if (!current) value = '';
	}
</script>

{#if favoriteId !== null}
	<select class="input group" aria-label="Group" bind:value onchange={changed}>
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
	{#if error}
		<span class="small bad" role="alert">{error}</span>
	{/if}
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
