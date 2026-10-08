<script lang="ts">
	let {
		samples,
		slots,
		label
	}: {
		/** Speeds, oldest first. */
		samples: number[];
		/** How many samples fit across; the newest sits on the right edge. */
		slots: number;
		label?: string;
	} = $props();

	const shown = $derived(samples.slice(-slots));
	// Newest sample on the right edge, so the line scrolls left as it fills.
	const xOf = (i: number) => (((slots - shown.length + i) / (slots - 1)) * 100).toFixed(2);
	const points = $derived.by(() => {
		const max = Math.max(1, ...shown);
		return shown.map((r, i) => `${xOf(i)},${(40 - (r / max) * 36).toFixed(2)}`).join(' ');
	});
</script>

<svg
	class="graph"
	viewBox="0 0 100 40"
	preserveAspectRatio="none"
	role={label ? 'img' : undefined}
	aria-label={label}
	aria-hidden={label ? undefined : 'true'}
>
	{#if shown.length > 1}
		<polygon class="graph-fill" points="{xOf(0)},40 {points} 100,40" />
		<polyline class="graph-line" {points} />
	{/if}
</svg>

<style>
	.graph {
		display: block;
		width: 100%;
		height: 100%;
	}
	.graph-fill {
		fill: var(--accent-soft);
	}
	.graph-line {
		fill: none;
		stroke: var(--accent);
		stroke-width: 1.5;
		vector-effect: non-scaling-stroke;
	}
</style>
