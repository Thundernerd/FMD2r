<script lang="ts">
	import type { Api } from '#lib/api/client.ts';
	import type { About, ToolCheck } from '#lib/api/types.ts';
	import { showInboxItem } from '#lib/inbox.svelte.ts';

	let { api }: { api: Api } = $props();

	let about = $state<About | null>(null);
	let loading = $state(false);
	let error = $state<string | null>(null);

	async function load() {
		loading = true;
		error = null;
		try {
			about = await api.about();
		} catch {
			error = 'Could not load the diagnostics.';
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		void load();
	});

	const formatBytes = (bytes: number | null | undefined): string => {
		if (bytes == null) return 'not created yet';
		const units = ['B', 'KB', 'MB', 'GB'];
		let value = bytes;
		let unit = 0;
		while (value >= 1024 && unit < units.length - 1) {
			value /= 1024;
			unit++;
		}
		return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
	};

	const formatUptime = (secs: number): string => {
		const d = Math.floor(secs / 86_400);
		const h = Math.floor((secs % 86_400) / 3600);
		const m = Math.floor((secs % 3600) / 60);
		return d > 0 ? `${d}d ${h}h ${m}m` : h > 0 ? `${h}h ${m}m` : `${m}m ${secs % 60}s`;
	};

	/** A short verdict for a tool check. */
	const verdict = (tool: ToolCheck): string => {
		if (tool.ok) return 'OK';
		if (tool.detail.includes('not found')) return 'Missing';
		return tool.name === 'FlareSolverr' ? 'Unreachable' : 'Failed';
	};

	function showInInbox(event: MouseEvent, id: string) {
		// Keep the inbox's outside-click handler from closing it again right away.
		event.stopPropagation();
		showInboxItem(id);
	}
</script>

<div class="about">
	<div class="head">
		<h2>Diagnostics</h2>
		<button class="btn sm" type="button" onclick={load} disabled={loading}>
			{loading ? 'Checking…' : 'Check again'}
		</button>
	</div>

	{#if error}
		<p class="error small" role="alert">{error}</p>
	{/if}

	{#if about}
		<table class="table">
			<tbody>
				<tr><th scope="row">Version</th><td class="mono">{about.version}</td></tr>
				<tr>
					<th scope="row">Git revision</th>
					<td class="mono">{about.git_revision ?? 'unknown'}</td>
				</tr>
				<tr>
					<th scope="row">Upstream Lua</th>
					<td class="mono">
						{about.upstream_ref ?? 'not synced'}{about.upstream_sha
							? ` @ ${about.upstream_sha}`
							: ''}
					</td>
				</tr>
				<tr><th scope="row">Modules loaded</th><td class="num">{about.module_count}</td></tr>
				<tr class:failed={about.load_failures.length > 0}>
					<th scope="row">Load failures</th>
					<td class="num">{about.load_failures.length}</td>
				</tr>
				<tr>
					<th scope="row">XPath backend</th>
					<td class="mono">{about.xpath_backend ?? 'unknown'}</td>
				</tr>
				<tr>
					<th scope="row">Data dir</th>
					<td class="mono">{about.data_dir ?? 'unknown'}</td>
				</tr>
				{#each about.databases as db (db.name)}
					<tr>
						<th scope="row">{db.name}</th>
						<td class="num">{formatBytes(db.bytes)}</td>
					</tr>
				{/each}
				<tr><th scope="row">Uptime</th><td class="num">{formatUptime(about.uptime_secs)}</td></tr>
			</tbody>
		</table>

		<h2>Tools</h2>
		<table class="table tools">
			<thead>
				<tr><th scope="col">Tool</th><th scope="col">Status</th><th scope="col">Detail</th></tr>
			</thead>
			<tbody>
				{#each about.tools as tool (tool.name)}
					<tr class:failed={!tool.ok}>
						<th scope="row" class="mono">{tool.name}</th>
						<td><span class="verdict">{verdict(tool)}</span></td>
						<td class="small detail">{tool.detail}</td>
					</tr>
				{/each}
			</tbody>
		</table>

		<h2>Module load failures</h2>
		{#if about.load_failures.length === 0}
			<p class="muted small">Every module loaded.</p>
		{:else}
			<ul class="failures">
				{#each about.load_failures as failure (failure.module)}
					<li>
						<b class="mono">{failure.module}</b>
						<span class="small detail">{failure.error}</span>
						{#if failure.inbox_id}
							<button
								class="btn sm"
								type="button"
								onclick={(e) => showInInbox(e, failure.inbox_id ?? '')}>Show in inbox</button
							>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	{:else if loading}
		<p class="muted">Running checks…</p>
	{/if}
</div>

<style>
	.about {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
		max-width: 760px;
	}
	.head {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
	}
	.head h2 {
		flex: 1;
	}
	h2 {
		font-size: var(--fs-lg);
	}
	.table {
		width: 100%;
		border-collapse: collapse;
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		overflow: hidden;
	}
	th,
	td {
		text-align: left;
		padding: 7px var(--sp-3);
		border-bottom: 1px solid var(--line);
		vertical-align: top;
	}
	tbody tr:last-child > * {
		border-bottom: 0;
	}
	tbody th {
		font-weight: 500;
		width: 34%;
	}
	.tools tbody th {
		width: auto;
	}
	thead th {
		font-size: var(--fs-xs);
		text-transform: uppercase;
		letter-spacing: 0.07em;
		color: var(--muted);
	}
	.detail {
		overflow-wrap: anywhere;
	}
	.verdict {
		font-size: var(--fs-xs);
		font-weight: 600;
		border-radius: var(--r-pill);
		padding: 1px 8px;
		background: var(--ok-soft);
		color: var(--ok);
	}
	tr.failed {
		background: var(--bad-soft);
	}
	tr.failed > * {
		color: var(--bad);
	}
	tr.failed .verdict {
		background: var(--bad);
		color: var(--surface);
	}
	.failures {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.failures li {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--sp-2);
		background: var(--surface);
		border: 1px solid var(--line);
		border-left: 3px solid var(--bad);
		border-radius: var(--r);
		padding: var(--sp-2) var(--sp-3);
	}
	.failures .detail {
		flex: 1 1 200px;
	}
	.error {
		margin: 0;
		color: var(--bad);
	}
</style>
