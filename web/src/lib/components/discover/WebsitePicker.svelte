<script lang="ts">
	import type { ModuleSummary } from '#lib/api/types.ts';

	let {
		modules,
		selected = $bindable()
	}: {
		modules: ModuleSummary[];
		/** Module ID; empty for every website. */
		selected: string;
	} = $props();

	let search = $state('');

	/** The modules matching the search, grouped by category, groups and modules by name. */
	const groups = $derived.by(() => {
		const words = search.toLowerCase().split(/\s+/).filter(Boolean);
		// The selected module stays listed so the select keeps showing it.
		const shown = modules.filter((m) => {
			const text = `${m.name} ${m.category}`.toLowerCase();
			return m.id === selected || words.every((w) => text.includes(w));
		});
		const byCategory = Object.groupBy(shown, (m) => m.category || 'Other');
		return Object.entries(byCategory)
			.sort(([a], [b]) => a.localeCompare(b))
			.map(([category, list = []]) => ({
				category,
				modules: list.toSorted((a, b) => a.name.localeCompare(b.name))
			}));
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
				{#each group.modules as m (m.id)}
					<option value={m.id}>{m.name}</option>
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
