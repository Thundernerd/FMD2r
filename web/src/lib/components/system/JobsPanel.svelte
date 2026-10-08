<script lang="ts">
	import { ApiError, type Api } from '#lib/api/client.ts';
	import type { JobPhase, JobState } from '#lib/api/types.ts';
	import type { EventStore } from '#lib/events.svelte.ts';

	let { api, store }: { api: Api; store: EventStore } = $props();

	/** Job ids in the server's order; jobs only seen on the event stream follow. */
	let order = $state<string[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	/** Jobs with a run/cancel request in flight. */
	let busy = $state<Record<string, boolean>>({});
	/** Why the last run/cancel request of a job failed. */
	let failures = $state<Record<string, string>>({});

	const jobs = $derived(
		[...order, ...Object.keys(store.jobs).filter((id) => !order.includes(id))].flatMap((id) => {
			const job = store.jobs[id];
			return job ? [job] : [];
		})
	);

	$effect(() => {
		api
			.listJobs()
			.then((list) => {
				store.seed({ jobs: list });
				order = list.map((j) => j.id);
			})
			.catch(() => (error = 'Could not load the jobs.'))
			.finally(() => (loading = false));
	});

	const PHASE: Record<JobPhase, string> = {
		idle: 'Idle',
		running: 'Running',
		done: 'Done',
		failed: 'Failed'
	};

	async function control(job: JobState, action: 'run' | 'cancel') {
		busy[job.id] = true;
		delete failures[job.id];
		try {
			store.updateJob(await (action === 'run' ? api.runJob(job.id) : api.cancelJob(job.id)));
		} catch (e) {
			failures[job.id] =
				e instanceof ApiError && e.status === 409
					? action === 'run'
						? 'Already running.'
						: 'Not running any more.'
					: `Could not ${action === 'run' ? 'start' : 'cancel'} the job.`;
		} finally {
			busy[job.id] = false;
		}
	}

	const percent = (job: JobState): number =>
		job.total > 0 ? Math.min(100, Math.round((job.done / job.total) * 100)) : 0;

	const formatWhen = (iso: string | null | undefined): string =>
		iso ? new Date(iso).toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' }) : '—';
</script>

{#if loading && jobs.length === 0}
	<p class="muted">Loading jobs…</p>
{:else if jobs.length === 0}
	<p class="placeholder">No background jobs are registered.</p>
{/if}

<div class="jobs">
	{#each jobs as job (job.id)}
		{@const running = job.state === 'running'}
		<article class="card state-{job.state}" aria-labelledby="job-{job.id}">
			<header>
				<h2 id="job-{job.id}">{job.title}</h2>
				<span class="badge">{PHASE[job.state]}</span>
			</header>

			{#if running}
				<div
					class="bar"
					role="progressbar"
					aria-label="{job.title} progress"
					aria-valuemin={0}
					aria-valuemax={job.total || undefined}
					aria-valuenow={job.total ? job.done : undefined}
				>
					<i class:indeterminate={!job.total} style:width="{job.total ? percent(job) : 30}%"></i>
				</div>
			{/if}
			{#if job.total > 0}
				<span class="small mono num">{job.done} / {job.total} ({percent(job)}%)</span>
			{/if}

			<dl class="small">
				<dt class="muted">Last run</dt>
				<dd>{formatWhen(job.last_run)}</dd>
				<dt class="muted">Next run</dt>
				<dd>{formatWhen(job.next_run)}</dd>
			</dl>

			{#if job.last_error}
				<details class="last-error">
					<summary class="small">Last error</summary>
					<pre class="mono">{job.last_error}</pre>
				</details>
			{/if}

			<div class="actions">
				<button
					class="btn sm primary"
					type="button"
					disabled={running || busy[job.id]}
					onclick={() => control(job, 'run')}>Run</button
				>
				<button
					class="btn sm"
					type="button"
					disabled={!running || busy[job.id]}
					onclick={() => control(job, 'cancel')}>Cancel</button
				>
			</div>
			{#if failures[job.id]}
				<p class="error small" role="alert">{failures[job.id]}</p>
			{/if}
		</article>
	{/each}
</div>

{#if error}
	<p class="error small" role="alert">{error}</p>
{/if}

<style>
	.jobs {
		display: flex;
		flex-wrap: wrap;
		align-items: flex-start;
		gap: var(--sp-3);
	}
	.card {
		flex: 1 1 280px;
		max-width: 420px;
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
		background: var(--surface);
		border: 1px solid var(--line);
		border-left: 3px solid var(--idle);
		border-radius: var(--r-lg);
		padding: var(--sp-3) var(--sp-4);
	}
	header {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
	}
	h2 {
		flex: 1;
		font-size: var(--fs-lg);
	}
	.badge {
		font-size: var(--fs-xs);
		font-weight: 600;
		border-radius: var(--r-pill);
		padding: 1px 8px;
		background: var(--idle-soft);
		color: var(--idle);
	}
	.state-running {
		border-left-color: var(--accent);
	}
	.state-running .badge {
		background: var(--accent-soft);
		color: var(--accent);
	}
	.state-done {
		border-left-color: var(--ok);
	}
	.state-done .badge {
		background: var(--ok-soft);
		color: var(--ok);
	}
	.state-failed {
		border-left-color: var(--bad);
	}
	.state-failed .badge {
		background: var(--bad-soft);
		color: var(--bad);
	}
	.bar .indeterminate {
		opacity: 0.5;
	}
	dl {
		display: flex;
		flex-wrap: wrap;
		gap: 2px var(--sp-2);
		margin: 0;
	}
	dt {
		width: 70px;
	}
	dd {
		margin: 0;
		width: calc(100% - 80px);
	}
	.last-error summary {
		cursor: pointer;
		color: var(--bad);
	}
	.last-error pre {
		white-space: pre-wrap;
		word-break: break-word;
		margin: var(--sp-1) 0 0;
		padding: var(--sp-2);
		background: var(--bad-soft);
		border-radius: var(--r);
	}
	.actions {
		display: flex;
		gap: var(--sp-2);
	}
	.btn:disabled {
		opacity: 0.5;
		cursor: default;
		border-color: var(--line);
	}
	.error {
		margin: 0;
		color: var(--bad);
	}
</style>
