<!-- PROTOTYPE (#224): a compact, read-only queue list for the S, C and R shells. -->
<script lang="ts">
	import SpeedGraph from '#lib/components/queue/SpeedGraph.svelte';
	import type { QueueStore } from '#lib/queue.svelte.ts';
	import { formatRate, percent } from '#lib/queue-format.ts';

	let {
		queue,
		limit = 99,
		graph = false,
		waitingList = false
	}: { queue: QueueStore; limit?: number; graph?: boolean; waitingList?: boolean } = $props();

	const waiting = $derived(queue.tasks.filter((t) => t.status === 'waiting'));
</script>

<div class="qmini">
	<div class="sum">
		<b class="mono num">{formatRate(queue.rate)}</b>
		<span class="small muted">{queue.active.length} active · {queue.waiting} waiting</span>
	</div>
	{#if graph}
		<div class="graph"><SpeedGraph samples={queue.history} slots={60} /></div>
	{/if}
	{#each queue.active.slice(0, limit) as task (task.id)}
		<div class="task">
			<div class="head small">
				<span class="title"><b>{task.title}</b> {task.chapters}</span>
				<span class="mono num">{task.done}/{task.total}</span>
			</div>
			<div class="bar"><i style:width="{percent(task.done, task.total)}%"></i></div>
		</div>
	{:else}
		<span class="small muted">Nothing downloading.</span>
	{/each}
	{#if waitingList && waiting.length > 0}
		<div class="label small muted">Waiting</div>
		{#each waiting as task (task.id)}
			<div class="small wait">{task.title} <span class="muted">{task.chapters}</span></div>
		{/each}
	{/if}
	<a class="btn sm" href="/queue">Open queue</a>
</div>

<style>
	.qmini {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
		min-width: 0;
	}
	.sum {
		display: flex;
		align-items: baseline;
		gap: var(--sp-2);
		flex-wrap: wrap;
	}
	.graph {
		height: 34px;
	}
	.task {
		display: flex;
		flex-direction: column;
		gap: 3px;
	}
	.head {
		display: flex;
		gap: var(--sp-2);
	}
	.title,
	.wait {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.bar {
		height: 5px;
		border-radius: var(--r-pill);
		background: var(--surface-2);
		overflow: hidden;
	}
	.bar i {
		display: block;
		height: 100%;
		background: var(--accent);
		transition: width 0.3s;
	}
	.label {
		margin-top: var(--sp-1);
		text-transform: uppercase;
		letter-spacing: 0.06em;
	}
	.btn {
		align-self: flex-start;
	}
</style>
