<script lang="ts">
	import type { ModuleSummary } from '#lib/api/types.ts';
	import { groupModules, moduleKey, moduleLabel, repeatedNames } from '#lib/modules.ts';

	let {
		modules,
		websites,
		selected = $bindable()
	}: {
		modules: ModuleSummary[];
		/** The IDs of the websites to list (`general.selected_websites`). */
		websites: string[];
		/** Module ID; empty for every selected website. */
		selected: string;
	} = $props();

	let search = $state('');

	const repeated = $derived(repeatedNames(modules));
	const label = (m: ModuleSummary) => moduleLabel(m, repeated);

	/** The selected websites matching the search, grouped by category. The picked one stays
	 * listed while the search hides it, so the select keeps showing it. */
	const groups = $derived.by(() => {
		const listed = new Set(websites);
		return groupModules(
			modules.filter((m) => listed.has(m.id)),
			search,
			(m) => m.id === selected
		);
	});
	const shown = $derived(groups.reduce((n, g) => n + g.modules.length, 0));
</script>

<div class="picker">
	<div class="head">
		<label class="label" for="website">Website</label>
		<a class="small" href="/settings#section-websites">Manage websites</a>
	</div>
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
	.head {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		gap: var(--sp-2);
	}
	p {
		margin: 0;
	}
</style>
