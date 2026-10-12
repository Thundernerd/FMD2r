<script lang="ts">
	import { ApiError, type Api } from '#lib/api/client.ts';
	import type { GroupView } from '#lib/api/types.ts';

	let {
		api,
		counts,
		onchange
	}: {
		api: Api;
		/** Series per group id. */
		counts: Record<number, number>;
		/** The groups changed; they are passed in their new order. */
		onchange: (groups: GroupView[]) => void;
	} = $props();

	let groups = $state<GroupView[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let busy = $state(false);
	let newName = $state('');
	/** The group being renamed, and the name typed so far. */
	let renaming = $state<{ id: number; name: string } | null>(null);
	/** The group whose deletion waits for confirmation. */
	let deleting = $state<GroupView | null>(null);

	const count = (id: number) => counts[id] ?? 0;

	async function reload() {
		groups = await api.listGroups();
		onchange(groups);
	}

	$effect(() => {
		api
			.listGroups()
			.then((list) => (groups = list))
			.catch(() => (error = 'Could not load the groups.'))
			.finally(() => (loading = false));
	});

	/** Runs a change, then reloads the groups; says why when the server refused it. */
	async function run(change: () => Promise<unknown>, name = '') {
		busy = true;
		error = null;
		try {
			await change();
			await reload();
			return true;
		} catch (e) {
			error =
				e instanceof ApiError && e.status === 409
					? `A group named ${name} exists already.`
					: e instanceof ApiError && e.status === 400
						? 'A group needs a name.'
						: 'Could not save the groups.';
			return false;
		} finally {
			busy = false;
		}
	}

	async function add(event: SubmitEvent) {
		event.preventDefault();
		const name = newName.trim();
		if (!name) return;
		if (await run(() => api.createGroup(name), name)) newName = '';
	}

	async function rename(event: SubmitEvent) {
		event.preventDefault();
		if (!renaming) return;
		const { id } = renaming;
		const name = renaming.name.trim();
		if (await run(() => api.updateGroup(id, { name }), name)) renaming = null;
	}

	function move(group: GroupView, by: -1 | 1) {
		const ids = groups.filter((g) => g.id !== group.id).map((g) => g.id);
		ids.splice(groups.indexOf(group) + by, 0, group.id);
		return run(() => api.reorderGroups(ids));
	}

	async function remove(group: GroupView) {
		if (await run(() => api.deleteGroup(group.id))) deleting = null;
	}
</script>

<div class="editor">
	{#if error}
		<p class="bad" role="alert">{error}</p>
	{/if}
	{#if loading}
		<p class="muted">Loading the groups…</p>
	{:else}
		{#if groups.length === 0}
			<p class="muted">
				No groups yet. Add one, then put series in it from their series pages or by selecting them
				in the Library; the Library shows each group as a section you can fold away.
			</p>
		{/if}
		<ul aria-label="Groups">
			{#each groups as group, index (group.id)}
				<li>
					{#if renaming?.id === group.id}
						<form class="row" onsubmit={rename}>
							<input
								class="input"
								aria-label="Name of {group.name}"
								bind:value={renaming.name}
								required
							/>
							<button class="btn sm primary" type="submit" disabled={busy}>Save</button>
							<button class="btn sm ghost" type="button" onclick={() => (renaming = null)}
								>Cancel</button
							>
						</form>
					{:else if deleting?.id === group.id}
						<div class="row">
							<span class="grow"
								>Delete {group.name}? Its {count(group.id)} series become ungrouped.</span
							>
							<button class="btn sm" type="button" disabled={busy} onclick={() => remove(group)}
								>Delete</button
							>
							<button class="btn sm ghost" type="button" onclick={() => (deleting = null)}
								>Cancel</button
							>
						</div>
					{:else}
						<div class="row">
							<span class="name grow"
								>{group.name} · <span class="num">{count(group.id)}</span></span
							>
							<button
								class="btn sm ghost"
								type="button"
								aria-label="Move {group.name} up"
								disabled={busy || index === 0}
								onclick={() => move(group, -1)}>↑</button
							>
							<button
								class="btn sm ghost"
								type="button"
								aria-label="Move {group.name} down"
								disabled={busy || index === groups.length - 1}
								onclick={() => move(group, 1)}>↓</button
							>
							<button
								class="btn sm ghost"
								type="button"
								aria-label="Rename {group.name}"
								onclick={() => {
									deleting = null;
									renaming = { id: group.id, name: group.name };
								}}>Rename</button
							>
							<button
								class="btn sm ghost"
								type="button"
								aria-label="Delete {group.name}"
								onclick={() => {
									renaming = null;
									deleting = group;
								}}>Delete</button
							>
						</div>
					{/if}
				</li>
			{/each}
		</ul>
		<form class="row" onsubmit={add}>
			<input
				class="input grow"
				aria-label="New group"
				placeholder="New group"
				bind:value={newName}
			/>
			<button class="btn sm" type="submit" disabled={busy || !newName.trim()}>Add</button>
		</form>
	{/if}
</div>

<style>
	.editor {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
		overflow-y: auto;
	}
	ul {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.row {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		flex-wrap: wrap;
	}
	.grow {
		flex: 1 1 160px;
		min-width: 0;
	}
	p {
		margin: 0;
	}
	.bad {
		color: var(--bad);
	}
</style>
