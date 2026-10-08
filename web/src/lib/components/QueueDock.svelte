<script lang="ts">
	import type { EventStore } from '#lib/events.svelte.ts';

	let { store }: { store: EventStore } = $props();

	const tasks = $derived(Object.values(store.tasks));
	const active = $derived(tasks.filter((t) => t.status === 'downloading'));
	const waiting = $derived(tasks.filter((t) => t.status === 'queued').length);
	const rate = $derived(active.reduce((sum, t) => sum + t.bytes_per_sec, 0) / 1_000_000);
	const percent = (done: number, total: number) =>
		total > 0 ? Math.round((done / total) * 100) : 0;
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
	<a class="btn sm primary" href="/queue">Open queue</a>
</section>

<style>
	.dock {
		position: fixed;
		left: var(--sp-4);
		right: var(--sp-4);
		bottom: calc(var(--sp-4) + var(--safe-bottom));
		margin: 0 auto;
		max-width: 900px;
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-xl);
		box-shadow: var(--shadow);
		z-index: 25;
		padding: 10px 14px;
		display: flex;
		gap: 14px;
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

	/* Phones: sit above the bottom tab bar and keep to one task line. */
	@media (max-width: 860px) {
		.dock {
			left: var(--sp-2);
			right: var(--sp-2);
			bottom: calc(var(--bottom-nav-h) + var(--safe-bottom) + var(--sp-2));
			padding: var(--sp-2) var(--sp-3);
			gap: var(--sp-3);
		}
		.task:not(:first-child) {
			display: none;
		}
		.summary .small {
			display: none;
		}
	}
</style>
