<script lang="ts">
	import type { EventStore } from '#lib/events.svelte.ts';

	let { store }: { store: EventStore } = $props();

	const SAMPLES = 60;

	const tasks = $derived(Object.values(store.tasks));
	const active = $derived(tasks.filter((t) => t.status === 'downloading'));
	const waiting = $derived(tasks.filter((t) => t.status === 'queued').length);
	const rate = $derived(active.reduce((sum, t) => sum + t.bytes_per_sec, 0) / 1_000_000);
	const percent = (done: number, total: number) =>
		total > 0 ? Math.round((done / total) * 100) : 0;

	// Total transfer rate over the last minute, sampled once a second.
	let history = $state<number[]>([]);
	$effect(() => {
		const timer = setInterval(() => {
			history = [...history, rate].slice(-SAMPLES);
		}, 1000);
		return () => clearInterval(timer);
	});
	// Newest sample on the right edge, so the line scrolls left as it fills.
	const xOf = (i: number) => (((SAMPLES - history.length + i) / (SAMPLES - 1)) * 100).toFixed(2);
	const points = $derived.by(() => {
		const max = Math.max(1, ...history);
		return history.map((r, i) => `${xOf(i)},${(40 - (r / max) * 36).toFixed(2)}`).join(' ');
	});
</script>

<section class="dock" aria-label="Download queue">
	<div class="tasks">
		{#each active as task (task.id)}
			<div class="task">
				<div class="task-head small">
					<span class="task-title"><b>{task.title}</b> {task.chapters}</span>
					<span class="mono num">{task.done}/{task.total}</span>
				</div>
				<div class="bar"><i style:width="{percent(task.done, task.total)}%"></i></div>
			</div>
		{:else}
			<span class="small muted">Nothing downloading.</span>
		{/each}
	</div>
	<div class="summary">
		<b class="mono num">{rate.toFixed(1)} MB/s</b>
		<span class="small muted">{active.length} active · {waiting} waiting</span>
	</div>
	<svg class="graph" viewBox="0 0 100 40" preserveAspectRatio="none" aria-hidden="true">
		{#if history.length > 1}
			<polygon class="graph-fill" points="{xOf(0)},40 {points} 100,40" />
			<polyline class="graph-line" {points} />
		{/if}
	</svg>
	<a class="btn sm primary" href="/queue">Open queue</a>
</section>

<style>
	/* Phones first: sit above the bottom tab bar and keep to one task line. */
	.dock {
		position: fixed;
		left: var(--sp-2);
		right: var(--sp-2);
		bottom: calc(var(--bottom-nav-h) + var(--safe-bottom) + var(--sp-2));
		margin: 0 auto;
		max-width: 900px;
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-xl);
		box-shadow: var(--shadow);
		z-index: 25;
		padding: var(--sp-2) var(--sp-3);
		display: flex;
		gap: var(--sp-3);
		align-items: center;
	}
	.tasks {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
	}
	.task {
		display: flex;
		flex-direction: column;
		gap: 3px;
	}
	.task:not(:first-child) {
		display: none;
	}
	.task-head {
		display: flex;
		gap: var(--sp-2);
	}
	.task-title {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.summary {
		display: flex;
		flex-direction: column;
	}
	.summary .small,
	.graph {
		display: none;
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

	@media (min-width: 861px) {
		.dock {
			left: var(--sp-4);
			right: var(--sp-4);
			bottom: calc(var(--sp-4) + var(--safe-bottom));
			padding: 10px 14px;
			gap: 14px;
		}
		.task:not(:first-child) {
			display: flex;
		}
		.summary .small {
			display: block;
		}
		.graph {
			display: block;
			width: 140px;
			height: 34px;
		}
	}
</style>
