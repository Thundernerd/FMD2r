<script lang="ts">
	import type { ModuleSummary } from '#lib/api/types.ts';
	import type { Draft } from '#lib/settings/draft.svelte.ts';
	import { moduleHost, moduleKey, repeatedNames } from '#lib/modules.ts';

	let {
		modules,
		draft
	}: {
		modules: ModuleSummary[];
		/** The settings draft whose `general.selected_websites` this edits. */
		draft: Draft<object>;
	} = $props();

	const PATH = 'general.selected_websites';

	let search = $state('');

	/** The selected module IDs, including those of modules that are not loaded. */
	const selection = $derived.by((): string[] => {
		const value = draft.get(PATH);
		return Array.isArray(value) ? value.filter((v) => typeof v === 'string') : [];
	});
	const selected = $derived(new Set(selection));
	const loaded = $derived(new Set(modules.map((m) => m.id)));
	const count = $derived([...loaded].filter((id) => selected.has(id)).length);

	const repeated = $derived(repeatedNames(modules));
	/** How a module is listed: by name, with its host when another module has that name too. */
	const label = (m: ModuleSummary) =>
		repeated.has(m.name) ? `${m.name} (${moduleHost(m)})` : m.name;

	/** The modules matching the search, grouped by category, groups and modules by label. */
	const groups = $derived.by(() => {
		const words = search.toLowerCase().split(/\s+/).filter(Boolean);
		const shown = modules.filter((m) => {
			const text = `${m.name} ${m.category}`.toLowerCase();
			return words.every((w) => text.includes(w));
		});
		const byCategory = Object.groupBy(shown, (m) => m.category || 'Other');
		return Object.entries(byCategory)
			.sort(([a], [b]) => a.localeCompare(b))
			.map(([category, list = []]) => ({
				category,
				modules: list.toSorted((a, b) => label(a).localeCompare(label(b)))
			}));
	});

	/** Selects or deselects `ids`, keeping the order of the rest. */
	function toggle(ids: string[], on: boolean) {
		const change = new Set(ids);
		draft.set(
			PATH,
			on
				? [...selection, ...[...change].filter((id) => !selected.has(id))]
				: selection.filter((id) => !change.has(id))
		);
	}

	const shownIds = () => groups.flatMap((g) => g.modules.map((m) => m.id));
</script>

<div class="websites">
	<p class="small muted">
		The websites Discover lists and searches. Adding by URL, the library and the series page work
		with every website.
	</p>
	<div class="bar">
		<input
			class="input"
			type="search"
			placeholder="Search websites"
			aria-label="Search websites"
			bind:value={search}
		/>
		<button class="btn sm" type="button" onclick={() => toggle(shownIds(), true)}>Select all</button
		>
		<button class="btn sm" type="button" onclick={() => toggle(shownIds(), false)}
			>Select none</button
		>
	</div>
	<p class="small muted" role="status">
		{count} of {loaded.size} websites selected
	</p>
	{#each groups as group (group.category)}
		<fieldset class="group">
			<legend class="label">{group.category}</legend>
			<!-- Modules sharing an ID share its selection, so either checkbox toggles it. -->
			{#each group.modules as m (moduleKey(m))}
				<label class="choice">
					<input
						type="checkbox"
						checked={selected.has(m.id)}
						onchange={(e) => toggle([m.id], e.currentTarget.checked)}
					/>
					<span>{label(m)}</span>
				</label>
			{/each}
		</fieldset>
	{:else}
		<p class="small muted">
			{search ? `No website matches “${search}”.` : 'No website modules are loaded.'}
		</p>
	{/each}
</div>

<style>
	.websites {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.bar {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-2);
		align-items: center;
	}
	.bar .input {
		flex: 1;
		min-width: 160px;
	}
	.group {
		border: 0;
		margin: 0;
		padding: 0;
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-1) var(--sp-4);
	}
	.group legend {
		padding: 0;
		margin-bottom: var(--sp-1);
	}
	.choice {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		min-width: 200px;
	}
	p {
		margin: 0;
	}
</style>
