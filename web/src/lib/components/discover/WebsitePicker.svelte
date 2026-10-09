<script lang="ts">
	import type { ModuleSummary } from '#lib/api/types.ts';
	import {
		groupModules,
		matchesSearch,
		moduleHost,
		moduleKey,
		repeatedNames
	} from '#lib/modules.ts';

	let {
		modules,
		selected = $bindable()
	}: {
		modules: ModuleSummary[];
		/** Module ID; empty for every website. */
		selected: string;
	} = $props();

	let search = $state('');

	const repeated = $derived(repeatedNames(modules));
	/** How a module is listed: by name, with its host when another module has that name too. */
	const label = (m: ModuleSummary) =>
		repeated.has(m.name) ? `${m.name} (${moduleHost(m)})` : m.name;

	/** The modules matching the search, grouped by category, groups and modules by name. */
	const groups = $derived.by(() => {
		// The selected module stays listed so the select keeps showing it.
		return groupModules(
			modules.filter((m) => m.id === selected || matchesSearch(`${m.name} ${m.category}`, search))
		);
	});
	const shown = $derived(groups.reduce((n, g) => n + g.modules.length, 0));
</script>

<div class="picker">
	<label class="label" for="website">Website</label>
	<input
		class="input"
		type="search"
		placeholder="Search websites"
		aria-label="Search websites"
		bind:value={search}
	/>
	<select id="website" class="input" bind:value={selected}>
		<option value="">All websites</option>
		{#each groups as group (group.category)}
			<optgroup label={group.category}>
				<!-- Modules sharing an ID share its list, so either option selects it. -->
				{#each group.modules as m (moduleKey(m))}
					<option value={m.id}>{label(m)}</option>
				{/each}
			</optgroup>
		{/each}
	</select>
	{#if search && shown === 0}
		<p class="small muted">No website matches “{search}”.</p>
	{/if}
</div>

<style>
	.picker {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	p {
		margin: 0;
	}
</style>
