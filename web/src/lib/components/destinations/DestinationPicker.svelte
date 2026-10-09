<script lang="ts">
	import type { Destination } from '#lib/api/types.ts';
	import { destinationAt } from '#lib/destinations/destinations.ts';

	let {
		destinations,
		value = $bindable(),
		label,
		id,
		inherit
	}: {
		destinations: Destination[];
		/** The folder picked: a destination's path, a custom folder, or `''` for `inherit`. */
		value: string;
		/** The picker's accessible name. */
		label: string;
		id?: string;
		/** When set, the first choice picks no folder (`''`), shown with this text. */
		inherit?: string;
	} = $props();

	/** Whether "Custom folder…" is picked, so the box shows even for a destination's path. */
	let custom = $state(false);

	const choice = $derived.by(() => {
		if (custom) return 'custom';
		if (inherit !== undefined && value.trim() === '') return 'inherit';
		const found = destinationAt(destinations, value);
		return found ? String(destinations.indexOf(found)) : 'custom';
	});

	function pick(next: string) {
		custom = next === 'custom';
		if (next === 'inherit') value = '';
		const destination = destinations[Number(next)];
		if (!custom && destination) value = destination.path;
	}
</script>

<div class="picker">
	<select
		{id}
		class="input"
		aria-label={label}
		value={choice}
		onchange={(e) => pick(e.currentTarget.value)}
	>
		{#if inherit !== undefined}
			<option value="inherit">{inherit}</option>
		{/if}
		{#each destinations as destination, i (i)}
			<option value={String(i)}>{destination.name}{destination.default ? ' (default)' : ''}</option>
		{/each}
		<option value="custom">Custom folder…</option>
	</select>
	{#if choice === 'custom'}
		<input
			class="input mono"
			type="text"
			autocomplete="off"
			aria-label="Custom folder"
			placeholder="/data/manga"
			bind:value
		/>
	{/if}
</div>

<style>
	.picker {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
		min-width: 0;
	}
</style>
