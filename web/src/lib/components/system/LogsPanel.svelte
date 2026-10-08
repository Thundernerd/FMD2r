<script lang="ts">
	import type { Api } from '#lib/api/client.ts';
	import type { LogLine } from '#lib/api/types.ts';
	import type { EventStore } from '#lib/events.svelte.ts';
	import { LEVELS, LogView, atBottom, formatLine, visibleRange } from '#lib/logs.svelte.ts';

	let { api, store }: { api: Api; store: EventStore } = $props();

	/** Every row is one line of this height, so the list can be virtualised. */
	const ROW_HEIGHT = 22;

	// The store is the app-wide singleton and never swapped, so reading it once is enough.
	// svelte-ignore state_referenced_locally
	const view = new LogView(store.logs);
	let follow = $state(true);
	let scroller: HTMLDivElement | undefined = $state();
	let scrollTop = $state(0);
	let height = $state(0);
	let error = $state<string | null>(null);
	let copied = $state<string | null>(null);
	/** The line shown in full below the list. */
	let selected = $state<LogLine | null>(null);

	const range = $derived(
		visibleRange({ scrollTop, height, rowHeight: ROW_HEIGHT, count: view.shown.length })
	);
	const rows = $derived(view.shown.slice(range.start, range.end));

	$effect(() => {
		// Lines logged before the stream connected, plus debug lines, which are never streamed.
		api
			.listLogs()
			.then((lines) => store.logs.add(lines))
			.catch(() => (error = 'Could not load the log.'));
	});

	// In follow mode, stick to the newest line whenever lines arrive or the filter changes.
	$effect(() => {
		void view.shown.length;
		if (!follow || !scroller) return;
		scroller.scrollTop = scroller.scrollHeight;
		scrollTop = scroller.scrollTop;
	});

	function onScroll() {
		if (!scroller) return;
		scrollTop = scroller.scrollTop;
		follow = atBottom(scroller);
	}

	function jumpToLatest() {
		follow = true;
		if (view.paused) view.resume();
	}

	async function copy(text: string, what: string) {
		try {
			await navigator.clipboard.writeText(text);
			copied = `Copied ${what}.`;
		} catch {
			copied = 'Could not copy: the browser blocked clipboard access.';
		}
		setTimeout(() => (copied = null), 2500);
	}

	const formatTime = (iso: string): string =>
		new Date(iso).toLocaleTimeString(undefined, { hour12: false });
</script>

<div class="logs">
	<div class="toolbar">
		<select class="input" aria-label="Level" bind:value={view.filter.level}>
			{#each LEVELS as level (level)}
				<option value={level}>{level === 'TRACE' ? 'All levels' : `${level} and up`}</option>
			{/each}
		</select>
		<select class="input" aria-label="Module" bind:value={view.filter.module}>
			<option value="">All modules</option>
			{#each view.modules as module (module)}
				<option value={module}>{module}</option>
			{/each}
		</select>
		<input
			class="input search"
			type="search"
			placeholder="Search"
			aria-label="Search logs"
			bind:value={view.filter.search}
		/>
		<div class="actions">
			{#if view.paused}
				<button class="btn sm primary" type="button" onclick={() => view.resume()}>
					Resume ({view.pending} new)
				</button>
			{:else}
				<button class="btn sm" type="button" onclick={() => view.pause()}>Pause</button>
			{/if}
			<button
				class="btn sm"
				type="button"
				onclick={() => copy(view.text(), `${view.shown.length} lines`)}>Copy</button
			>
		</div>
	</div>

	<div class="frame">
		<div
			class="scroller mono"
			role="log"
			aria-label="Server log"
			aria-live="off"
			bind:this={scroller}
			bind:clientHeight={height}
			onscroll={onScroll}
		>
			<div class="spacer" style:height="{view.shown.length * ROW_HEIGHT}px">
				<div class="rows" style:transform="translateY({range.start * ROW_HEIGHT}px)">
					{#each rows as line (line.seq)}
						<button
							type="button"
							class="row lvl-{line.level.toLowerCase()}"
							class:selected={selected?.seq === line.seq}
							title={line.message}
							onclick={() => (selected = selected?.seq === line.seq ? null : line)}
						>
							<span class="time muted">{formatTime(line.time)}</span>
							<span class="level">{line.level}</span>
							{#if line.module}<span class="module">{line.module}</span>{/if}
							<span class="target muted">{line.target}</span>
							<span class="message">{line.message}</span>
						</button>
					{/each}
				</div>
			</div>
		</div>
		{#if view.shown.length === 0}
			<p class="empty muted">No log lines match.</p>
		{/if}
		{#if !follow}
			<button class="btn sm primary latest" type="button" onclick={jumpToLatest}>
				Jump to latest
			</button>
		{/if}
	</div>

	{#if selected}
		<div class="detail">
			<pre class="mono">{formatLine(selected)}</pre>
			<button
				class="btn sm"
				type="button"
				onclick={() => selected && copy(formatLine(selected), 'the line')}>Copy line</button
			>
		</div>
	{/if}

	<p class="status small muted" aria-live="polite">
		{view.shown.length} of {store.logs.lines.length} lines{view.paused ? ' · paused' : ''}
		{#if copied}· {copied}{/if}
	</p>
	{#if error}
		<p class="error small" role="alert">{error}</p>
	{/if}
</div>

<style>
	.logs {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.toolbar {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-2);
		align-items: center;
	}
	.search {
		flex: 1 1 160px;
	}
	.actions {
		display: flex;
		gap: var(--sp-2);
	}
	.frame {
		position: relative;
	}
	/* Fill the viewport down to the queue dock (and, on phones, the tab bar). */
	.scroller {
		height: calc(100dvh - 440px);
		min-height: 240px;
		overflow: auto;
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		overscroll-behavior: contain;
	}
	.spacer {
		position: relative;
	}
	.rows {
		position: absolute;
		top: 0;
		left: 0;
		right: 0;
		will-change: transform;
	}
	.row {
		width: 100%;
		height: 22px;
		line-height: 22px;
		padding: 0 var(--sp-2);
		display: flex;
		gap: var(--sp-2);
		white-space: nowrap;
		overflow: hidden;
		border: 0;
		border-left: 3px solid transparent;
		background: transparent;
		font: inherit;
		text-align: left;
	}
	.row:hover {
		background: var(--surface-2);
	}
	.row.selected {
		background: var(--accent-soft);
	}
	.row > span {
		flex: none;
	}
	.row .message {
		flex: 1 1 auto;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.level {
		width: 5ch;
		font-weight: 500;
	}
	.module {
		color: var(--accent);
	}
	.lvl-error {
		border-left-color: var(--bad);
		background: var(--bad-soft);
	}
	.lvl-error .level {
		color: var(--bad);
	}
	.lvl-warn {
		border-left-color: var(--warn);
	}
	.lvl-warn .level {
		color: var(--warn);
	}
	.lvl-debug,
	.lvl-trace {
		color: var(--muted);
	}
	.empty {
		position: absolute;
		inset: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		margin: 0;
		pointer-events: none;
	}
	.latest {
		position: absolute;
		right: var(--sp-3);
		bottom: var(--sp-3);
		box-shadow: var(--shadow);
	}
	.detail {
		display: flex;
		gap: var(--sp-2);
		align-items: flex-start;
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r);
		padding: var(--sp-2);
	}
	.detail pre {
		flex: 1;
		margin: 0;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
	.status,
	.error {
		margin: 0;
	}
	.error {
		color: var(--bad);
	}
	/* Phones: drop the target column so the message keeps some room. */
	@media (min-width: 861px) {
		.scroller {
			height: calc(100dvh - 420px);
		}
	}
	@media (max-width: 600px) {
		.target {
			display: none;
		}
	}
</style>
