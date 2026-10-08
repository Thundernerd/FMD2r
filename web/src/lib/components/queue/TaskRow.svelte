<script lang="ts">
	import type { Api, TaskAction } from '#lib/api/client.ts';
	import type { TaskState, TaskSummary } from '#lib/api/types.ts';
	import { groupOf, type QueueStore } from '#lib/queue.svelte.ts';
	import { formatRate, percent } from '#lib/queue-format.ts';
	import { seriesHref } from '#lib/series/href.ts';

	let {
		task,
		api,
		queue,
		onerror
	}: {
		task: TaskSummary;
		api: Api;
		queue: QueueStore;
		/** An action failed; `message` says which. */
		onerror: (message: string) => void;
	} = $props();

	const STATUS: Record<TaskState, string> = {
		stopped: 'Stopped',
		waiting: 'Waiting',
		preparing: 'Preparing',
		downloading: 'Downloading',
		converting: 'Converting',
		compressing: 'Compressing',
		finished: 'Finished',
		failed: 'Failed',
		disabled: 'Disabled'
	};

	let busy = $state(false);
	let confirming = $state(false);

	const group = $derived(groupOf(task.status));
	// FMD2's rules: start a stopped or failed task, stop a waiting or running one
	// (baseunits/uDownloadsManager.pas:1835-1844, :1900-1920).
	const canStart = $derived(
		task.enabled && (task.status === 'stopped' || task.status === 'failed')
	);
	const canStop = $derived(group === 'downloading' || group === 'waiting');
	const canRedownload = $derived(task.enabled && !task.running && task.status !== 'waiting');
	const hasFiles = $derived(task.chapters_done > 0);

	async function run(action: TaskAction) {
		if (busy) return;
		busy = true;
		try {
			queue.upsert(await api.taskAction(task.id, action));
		} catch {
			onerror(`Could not ${action} “${task.title}”.`);
		} finally {
			busy = false;
		}
	}

	async function remove(files: boolean) {
		if (busy) return;
		busy = true;
		try {
			await api.deleteTask(task.id, files);
			queue.removed(task.id);
		} catch {
			onerror(`Could not delete “${task.title}”.`);
		} finally {
			busy = false;
			confirming = false;
		}
	}
</script>

<li class="task status-{task.status}" aria-label={task.title}>
	<div class="head">
		<span class="title">{task.title}</span>
		<span class="badge small">{STATUS[task.status]}</span>
	</div>
	<div class="meta small muted">
		<span>{task.chapters}</span>
		<span>{task.chapters_done}/{task.chapter_count} chapters</span>
		{#if task.total > 0}
			<span class="mono num">{task.done}/{task.total} pages</span>
		{/if}
		{#if group === 'downloading'}
			<span class="mono num">{formatRate(task.bytes_per_sec)}</span>
		{/if}
		<span>{task.module_id}</span>
	</div>
	{#if group === 'downloading' || task.total > 0}
		<div
			class="bar"
			role="progressbar"
			aria-label="Pages of the current chapter"
			aria-valuemin={0}
			aria-valuemax={task.total}
			aria-valuenow={task.done}
		>
			<i style:width="{percent(task.done, task.total)}%"></i>
		</div>
	{/if}
	{#if task.error}
		<p class="error small">{task.error}</p>
	{/if}
	<div class="actions">
		{#if canStart}
			<button class="btn sm" type="button" disabled={busy} onclick={() => run('start')}>
				{task.status === 'failed' ? 'Retry' : 'Start'}
			</button>
		{/if}
		{#if canStop}
			<button class="btn sm" type="button" disabled={busy} onclick={() => run('stop')}>Stop</button>
		{/if}
		{#if hasFiles}
			<a class="btn sm" href={api.taskFilesUrl(task.id)} download>Get files</a>
		{/if}
		{#if canRedownload}
			<button class="btn sm" type="button" disabled={busy} onclick={() => run('redownload')}>
				Download again
			</button>
		{/if}
		<button
			class="btn sm"
			type="button"
			disabled={busy}
			onclick={() => run(task.enabled ? 'disable' : 'enable')}
		>
			{task.enabled ? 'Disable' : 'Enable'}
		</button>
		<a class="btn sm ghost" href={seriesHref(task)}>Open series</a>
		{#if confirming}
			<span class="confirm" role="group" aria-label="Delete {task.title}">
				<button class="btn sm danger" type="button" disabled={busy} onclick={() => remove(false)}>
					Delete task
				</button>
				<button class="btn sm danger" type="button" disabled={busy} onclick={() => remove(true)}>
					Delete with files
				</button>
				<button class="btn sm ghost" type="button" onclick={() => (confirming = false)}>
					Cancel
				</button>
			</span>
		{:else}
			<button
				class="btn sm ghost"
				type="button"
				disabled={busy}
				onclick={() => (confirming = true)}
			>
				Delete…
			</button>
		{/if}
	</div>
</li>

<style>
	.task {
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding: 10px 12px;
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		min-width: 0;
	}
	.head {
		display: flex;
		align-items: baseline;
		gap: var(--sp-2);
		min-width: 0;
	}
	.title {
		flex: 1;
		min-width: 0;
		font-weight: 600;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.badge {
		flex: none;
		padding: 1px 8px;
		border-radius: var(--r-pill);
		background: var(--idle-soft);
		color: var(--idle);
		font-weight: 600;
	}
	.status-preparing .badge,
	.status-downloading .badge,
	.status-converting .badge,
	.status-compressing .badge {
		background: var(--accent-soft);
		color: var(--accent);
	}
	.status-finished .badge {
		background: var(--ok-soft);
		color: var(--ok);
	}
	.status-failed .badge {
		background: var(--bad-soft);
		color: var(--bad);
	}
	.status-waiting .badge {
		background: var(--warn-soft);
		color: var(--warn);
	}
	.meta {
		display: flex;
		flex-wrap: wrap;
		column-gap: var(--sp-3);
		row-gap: 2px;
	}
	.error {
		margin: 0;
		color: var(--bad);
		overflow-wrap: anywhere;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-1);
	}
	.confirm {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-1);
	}
	.btn.danger {
		color: var(--bad);
		border-color: var(--bad);
	}
	.btn:disabled {
		opacity: 0.55;
		cursor: default;
	}
</style>
