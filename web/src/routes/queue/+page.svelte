<script lang="ts">
	import { api, events } from '#lib/app.ts';
	import SpeedGraph from '#lib/components/queue/SpeedGraph.svelte';
	import TaskRow from '#lib/components/queue/TaskRow.svelte';
	import { GROUPS } from '#lib/queue.svelte.ts';
	import { formatRate } from '#lib/queue-format.ts';

	const queue = events.queue;

	/** The page's graph spans five minutes of one sample a second. */
	const SLOTS = 300;

	let error = $state<string | null>(null);
	let busy = $state(false);

	async function bulk(what: string, call: () => Promise<void>) {
		if (busy) return;
		busy = true;
		error = null;
		try {
			await call();
			queue.resync();
		} catch {
			error = `Could not ${what}.`;
		} finally {
			busy = false;
		}
	}

	const total = $derived(queue.matching.length);
	const filtering = $derived(
		queue.filter.text !== '' ||
			queue.filter.status !== 'all' ||
			queue.filter.from !== '' ||
			queue.filter.to !== ''
	);

	function clearFilter() {
		queue.filter = { text: '', status: 'all', from: '', to: '' };
	}
</script>

<svelte:head><title>Queue · FMD2r</title></svelte:head>

<div class="page">
	<div class="title-row">
		<h1>Queue</h1>
		<div class="toolbar">
			<button
				class="btn sm"
				type="button"
				disabled={busy}
				onclick={() => bulk('start all tasks', api.startAllTasks)}>Start all</button
			>
			<button
				class="btn sm"
				type="button"
				disabled={busy}
				onclick={() => bulk('stop all tasks', api.stopAllTasks)}>Stop all</button
			>
			<button
				class="btn sm"
				type="button"
				disabled={busy || queue.counts.finished === 0}
				onclick={() => bulk('remove the finished tasks', api.removeFinishedTasks)}
				>Remove finished</button
			>
		</div>
	</div>

	<section class="speed" aria-label="Download speed">
		<div class="speed-text">
			<b class="mono num rate">{formatRate(queue.rate)}</b>
			<span class="small muted">{queue.active.length} active · {queue.waiting} waiting</span>
		</div>
		<div class="speed-graph">
			<SpeedGraph samples={queue.history} slots={SLOTS} label="Download speed, last 5 minutes" />
		</div>
	</section>

	<div class="filters" role="search" aria-label="Filter tasks">
		<input
			class="input text"
			type="search"
			placeholder="Filter by title, website or chapter"
			aria-label="Filter text"
			bind:value={queue.filter.text}
		/>
		<div class="chips" role="group" aria-label="Status">
			<button
				class="chip"
				type="button"
				aria-pressed={queue.filter.status === 'all'}
				onclick={() => (queue.filter.status = 'all')}
			>
				All <span class="num">{total}</span>
			</button>
			{#each GROUPS as group (group.key)}
				<button
					class="chip"
					type="button"
					aria-pressed={queue.filter.status === group.key}
					onclick={() => (queue.filter.status = group.key)}
				>
					{group.label} <span class="num">{queue.counts[group.key]}</span>
				</button>
			{/each}
		</div>
		<div class="dates">
			<label class="date">
				<span class="label">From</span>
				<input class="input" type="date" bind:value={queue.filter.from} />
			</label>
			<label class="date">
				<span class="label">To</span>
				<input class="input" type="date" bind:value={queue.filter.to} />
			</label>
			{#if filtering}
				<button class="btn sm ghost" type="button" onclick={clearFilter}>Clear filter</button>
			{/if}
		</div>
	</div>

	{#if error}
		<p class="problem small" role="alert">{error}</p>
	{/if}

	{#if queue.tasks.length === 0}
		<p class="muted">The queue is empty. Queue chapters from a series page.</p>
	{:else}
		{#each queue.groups as group (group.key)}
			<section class="group" aria-labelledby="group-{group.key}">
				<h2 id="group-{group.key}">
					{group.label} <span class="count num muted">{group.tasks.length}</span>
				</h2>
				{#if group.tasks.length === 0}
					<p class="small muted">{filtering ? 'No matching tasks.' : 'None.'}</p>
				{:else}
					<ul class="tasks">
						{#each group.tasks as task (task.id)}
							<TaskRow {task} {api} {queue} onerror={(message) => (error = message)} />
						{/each}
					</ul>
				{/if}
			</section>
		{/each}
	{/if}
</div>

<style>
	.title-row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: var(--sp-2);
	}
	.title-row h1 {
		font-size: var(--fs-h1);
		letter-spacing: -0.02em;
	}
	.toolbar {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-1);
	}
	.speed {
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		padding: 10px 14px;
	}
	.speed-text {
		display: flex;
		flex-direction: column;
		flex: none;
	}
	.rate {
		font-size: var(--fs-lg);
	}
	.speed-graph {
		flex: 1;
		min-width: 0;
		height: 48px;
	}
	.filters {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.text {
		width: 100%;
	}
	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-1);
	}
	.chip {
		border: 1px solid var(--line);
		background: var(--surface);
		border-radius: var(--r-pill);
		padding: 3px 10px;
		font-size: var(--fs-sm);
		color: var(--fg);
	}
	.chip[aria-pressed='true'] {
		background: var(--accent);
		border-color: var(--accent);
		color: var(--accent-fg);
	}
	.dates {
		display: flex;
		flex-wrap: wrap;
		align-items: flex-end;
		gap: var(--sp-2);
	}
	.date {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.problem {
		margin: 0;
		padding: 6px 10px;
		border-radius: var(--r);
		background: var(--bad-soft);
		color: var(--bad);
	}
	.group {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.group h2 {
		font-size: var(--fs-md);
		margin: 0;
	}
	.count {
		font-weight: 400;
	}
	.tasks {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}

	@media (min-width: 861px) {
		.filters {
			flex-direction: row;
			flex-wrap: wrap;
			align-items: flex-end;
		}
		.text {
			width: 280px;
		}
	}
</style>
