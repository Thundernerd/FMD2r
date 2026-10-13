<script lang="ts">
	import { ApiError, type Api } from '#lib/api/client.ts';
	import type { GroupView, RuleView } from '#lib/api/types.ts';
	import GroupRuleFields, { describeRule, ruleOf } from './GroupRuleFields.svelte';

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
	/** The add form, or the group being edited: its name and rule as typed so far. */
	interface Draft {
		name: string;
		kind: RuleView['kind'] | '';
		tag: string;
		format: string;
	}
	const draftOf = (group: GroupView | null): Draft => ({
		name: group?.name ?? '',
		kind: group?.rule?.kind ?? '',
		tag: group?.rule?.kind === 'tag' ? group.rule.value : '',
		format: group?.rule?.kind === 'format' ? group.rule.value : 'manga'
	});
	const ruleOfDraft = (d: Draft) => ruleOf(d.kind, d.kind === 'tag' ? d.tag : d.format);
	let adding = $state(draftOf(null));
	/** The group being edited, and its draft. */
	let editing = $state<({ id: number } & Draft) | null>(null);
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
						? 'A group needs a name, and a tag rule a tag.'
						: 'Could not save the groups.';
			return false;
		} finally {
			busy = false;
		}
	}

	async function add(event: SubmitEvent) {
		event.preventDefault();
		const name = adding.name.trim();
		if (!name) return;
		const rule = ruleOfDraft(adding);
		if (await run(() => api.createGroup(name, rule), name)) adding = draftOf(null);
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (!editing) return;
		const { id } = editing;
		const name = editing.name.trim();
		const rule = ruleOfDraft(editing);
		if (await run(() => api.updateGroup(id, { name, rule }), name)) editing = null;
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
				in the Library, or let it collect the series without a group by tag or format. The Library
				shows each group as a section you can fold away.
			</p>
		{/if}
		<ul aria-label="Groups">
			{#each groups as group, index (group.id)}
				<li>
					{#if editing?.id === group.id}
						<form class="row" onsubmit={save}>
							<input
								class="input"
								aria-label="Name of {group.name}"
								bind:value={editing.name}
								required
							/>
							<GroupRuleFields
								bind:kind={editing.kind}
								bind:tag={editing.tag}
								bind:format={editing.format}
								of={group.name}
							/>
							<button class="btn sm primary" type="submit" disabled={busy}>Save</button>
							<button class="btn sm ghost" type="button" onclick={() => (editing = null)}
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
								>{group.name} · <span class="num">{count(group.id)}</span>{group.rule
									? ` · ${describeRule(group.rule)}`
									: ''}</span
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
								aria-label="Edit {group.name}"
								onclick={() => {
									deleting = null;
									editing = { id: group.id, ...draftOf(group) };
								}}>Edit</button
							>
							<button
								class="btn sm ghost"
								type="button"
								aria-label="Delete {group.name}"
								onclick={() => {
									editing = null;
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
				bind:value={adding.name}
			/>
			<GroupRuleFields bind:kind={adding.kind} bind:tag={adding.tag} bind:format={adding.format} />
			<button class="btn sm" type="submit" disabled={busy || !adding.name.trim()}>Add</button>
		</form>
	{/if}
</div>

<style>
	.editor {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
		overflow-y: auto;
		/* Scrolling clips on every side. Leave room for the focus outline (outline + 1px offset),
		   pulling the edges back out so the dialog's layout doesn't move, and keep that room where
		   focus scrolls a control into view (1px more, as scrolling snaps to whole pixels). */
		--ring: calc(var(--focus-w) + 1px);
		padding: var(--ring);
		margin: calc(-1 * var(--ring));
		scroll-padding: calc(var(--ring) + 1px);
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
