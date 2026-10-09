<script lang="ts">
	import type { Destination, FolderCheck } from '#lib/api/types.ts';
	import { freeName } from '#lib/destinations/destinations.ts';
	import type { Draft, Json } from '#lib/settings/draft.svelte.ts';

	let {
		draft,
		checkFolders
	}: {
		/** The settings draft; this edits its `saveto.destinations`. */
		draft: Draft<object>;
		/** Whether downloads can be saved in each folder now. */
		checkFolders: (paths: string[]) => Promise<FolderCheck[]>;
	} = $props();

	const PATH = 'saveto.destinations';
	const uid = $props.id();

	const list = $derived((draft.get(PATH) ?? []) as unknown as Destination[]);
	const dirty = $derived(draft.isDirty(PATH));
	const listError = $derived(draft.errors[PATH]);
	const invalid = $derived(Object.keys(draft.errors).some((k) => k.startsWith(PATH)));

	/** Stores `next` as the destinations, dropping the errors of the rows it replaces. */
	function update(next: Destination[]) {
		for (const key of Object.keys(draft.errors)) {
			if (key.startsWith(`${PATH}.`)) delete draft.errors[key];
		}
		draft.set(PATH, next as unknown as Json);
	}

	const edit = (i: number, change: Partial<Destination>) =>
		update(list.map((d, j) => (j === i ? { ...d, ...change } : d)));

	function add() {
		update([...list, { name: freeName(list, 'New destination'), path: '', default: false }]);
	}

	const makeDefault = (i: number) => update(list.map((d, j) => ({ ...d, default: j === i })));

	function move(i: number, by: -1 | 1) {
		const next = [...list];
		const [moved] = next.splice(i, 1);
		if (!moved) return;
		next.splice(i + by, 0, moved);
		update(next);
	}

	const remove = (i: number) => update(list.filter((_, j) => j !== i));

	/** Why each folder can't take downloads now, by path. */
	let problems = $state<Record<string, string>>({});
	// Checked as they are typed, debounced; a missing disk is a warning, not an error.
	$effect(() => {
		const paths = [...new Set(list.map((d) => d.path.trim()).filter(Boolean))];
		const timer = setTimeout(() => {
			checkFolders(paths)
				.then((checks) => {
					problems = Object.fromEntries(
						checks.flatMap((c) => (c.problem ? [[c.path, c.problem]] : []))
					);
				})
				.catch(() => (problems = {}));
		}, 300);
		return () => clearTimeout(timer);
	});
</script>

<div class="field destinations" class:dirty class:invalid data-path={PATH}>
	<span class="caption">Download destinations</span>
	<p class="help small muted">
		The folders downloads can go to. A download goes to the folder picked on the series page, else
		the website's own destination, else the default one.
	</p>
	<ul class="rows">
		{#each list as destination, i (i)}
			{@const id = `${uid}-${i}`}
			{@const nameError = draft.errors[`${PATH}.${i}.name`]}
			{@const pathError = draft.errors[`${PATH}.${i}.path`]}
			{@const problem = problems[destination.path.trim()]}
			<li
				class="row"
				class:invalid={!!(nameError || pathError)}
				role="group"
				aria-label={destination.name || 'Unnamed destination'}
			>
				<div class="inputs">
					<label class="part name">
						<span class="label">Name</span>
						<input
							class="input"
							type="text"
							autocomplete="off"
							value={destination.name}
							aria-invalid={nameError ? true : undefined}
							aria-describedby={nameError ? `${id}-name-error` : undefined}
							oninput={(e) => edit(i, { name: e.currentTarget.value })}
						/>
					</label>
					<label class="part path">
						<span class="label">Folder</span>
						<input
							class="input mono"
							type="text"
							autocomplete="off"
							value={destination.path}
							aria-invalid={pathError ? true : undefined}
							aria-describedby={[pathError ? `${id}-path-error` : '', problem ? `${id}-warn` : '']
								.filter(Boolean)
								.join(' ') || undefined}
							oninput={(e) => edit(i, { path: e.currentTarget.value })}
						/>
					</label>
				</div>
				<div class="actions">
					<label class="check small">
						<input
							type="radio"
							name="{uid}-default"
							checked={destination.default}
							onchange={() => makeDefault(i)}
						/>
						Default
					</label>
					<button
						class="btn sm ghost"
						type="button"
						aria-label="Move up"
						title="Move up"
						disabled={i === 0}
						onclick={() => move(i, -1)}>↑</button
					>
					<button
						class="btn sm ghost"
						type="button"
						aria-label="Move down"
						title="Move down"
						disabled={i === list.length - 1}
						onclick={() => move(i, 1)}>↓</button
					>
					<button
						class="btn sm"
						type="button"
						disabled={destination.default}
						title={destination.default ? "The default destination can't be removed" : undefined}
						onclick={() => remove(i)}>Remove</button
					>
				</div>
				{#if nameError}
					<p id="{id}-name-error" class="field-error small" role="alert">{nameError}</p>
				{/if}
				{#if pathError}
					<p id="{id}-path-error" class="field-error small" role="alert">{pathError}</p>
				{/if}
				{#if problem && !pathError}
					<p id="{id}-warn" class="warning small">
						Downloads can't be saved here now: {problem}. They will be once it is available.
					</p>
				{/if}
			</li>
		{/each}
	</ul>
	{#if listError}
		<p class="field-error small" role="alert">{listError}</p>
	{/if}
	<div>
		<button class="btn sm" type="button" onclick={add}>Add destination</button>
	</div>
</div>

<style>
	.field {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
		padding: var(--sp-2) 0 var(--sp-2) var(--sp-3);
		border-left: 2px solid transparent;
	}
	.field.dirty {
		border-left-color: var(--accent);
	}
	.field.invalid {
		border-left-color: var(--bad);
	}
	.caption {
		font-weight: 500;
	}
	.help {
		margin: 0;
	}
	.rows {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.row {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
		padding: var(--sp-2) var(--sp-3);
		border: 1px solid var(--line);
		border-radius: var(--r);
	}
	.row.invalid {
		border-color: var(--bad);
	}
	.inputs {
		display: flex;
		gap: var(--sp-2);
		flex-wrap: wrap;
	}
	.part {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
		min-width: 0;
	}
	.part.name {
		flex: 1 1 140px;
	}
	.part.path {
		flex: 3 1 220px;
	}
	.actions {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		flex-wrap: wrap;
	}
	.check {
		display: flex;
		align-items: center;
		gap: var(--sp-1);
		margin-right: auto;
	}
	.btn:disabled {
		opacity: 0.55;
		cursor: default;
	}
	.field-error,
	.warning {
		margin: 0;
	}
	.field-error {
		color: var(--bad);
	}
	.warning {
		color: var(--warn);
	}
</style>
