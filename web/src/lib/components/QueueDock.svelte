<script lang="ts">
	import SpeedGraph from '#lib/components/queue/SpeedGraph.svelte';
	import type { QueueStore } from '#lib/queue.svelte.ts';
	import { formatRate, percent } from '#lib/queue-format.ts';

	let { queue }: { queue: QueueStore } = $props();

	/** The dock's graph spans the last minute. */
	const SLOTS = 60;
</script>

<section class="dock" aria-label="Download queue">
	<div class="tasks">
		{#each queue.active as task (task.id)}
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
		<b class="mono num">{formatRate(queue.rate)}</b>
		<span class="small muted">{queue.active.length} active · {queue.waiting} waiting</span>
	</div>
	<div class="graph">
		<SpeedGraph samples={queue.history} slots={SLOTS} />
	</div>
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
