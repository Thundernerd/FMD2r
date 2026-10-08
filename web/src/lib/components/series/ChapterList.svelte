<script lang="ts">
	import type { ChapterInfo } from '#lib/api/types.ts';
	import {
		displayOrder,
		extendRange,
		parseRanges,
		selectAll,
		selectNew,
		toggle
	} from '#lib/series/selection.ts';

	let { chapters, selected = $bindable() }: { chapters: ChapterInfo[]; selected: Set<number> } =
		$props();

	/** Row height in px; rows are fixed-height so only the visible ones need rendering. */
	const ROW = 36;
	/** Rows rendered above and below the visible ones, so fast scrolling shows no gaps. */
	const OVERSCAN = 10;

	let newestFirst = $state(false);
	let hideDownloaded = $state(false);
	let range = $state('');
	let rangeError = $state(false);
	/** The chapter a shift-click extends from. */
	let anchor = $state<number | null>(null);

	let scrollTop = $state(0);
	let viewport = $state(0);

	const order = $derived(
		displayOrder(chapters.length, newestFirst).filter(
			(i) => !hideDownloaded || !chapters[i]?.downloaded
		)
	);
	const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
	const last = $derived(Math.min(order.length, Math.ceil((scrollTop + viewport) / ROW) + OVERSCAN));
	const visible = $derived(order.slice(first, last));
	/** FMD2's `0001 - name` numbering: the position in module order (frmMain.pas:3177). */
	const width = $derived(Math.max(4, String(chapters.length).length));
	const number = (i: number) => String(i + 1).padStart(width, '0');

	function pick(event: MouseEvent, index: number) {
		selected =
			event.shiftKey && anchor !== null
				? extendRange(selected, order, anchor, index)
				: toggle(selected, index);
		anchor = index;
	}

	function applyRange(event: SubmitEvent) {
		event.preventDefault();
		const picked = parseRanges(range, chapters.length);
		rangeError = picked === null;
		if (picked) selected = picked;
	}
</script>

<section class="chapters" aria-label="Chapters">
	<header>
		<h2>Chapters</h2>
		<span class="small muted num">{chapters.length}</span>
		<button class="btn sm" type="button" onclick={() => (newestFirst = !newestFirst)}>
			{newestFirst ? 'Newest first' : 'Oldest first'}
		</button>
	</header>
	<div class="tools">
		<div class="quick" role="group" aria-label="Select">
			<button class="btn sm" type="button" onclick={() => (selected = selectAll(chapters.length))}>
				All
			</button>
			<button class="btn sm" type="button" onclick={() => (selected = selectNew(chapters))}>
				New
			</button>
			<button class="btn sm" type="button" onclick={() => (selected = new Set())}>None</button>
		</div>
		<form class="range" onsubmit={applyRange}>
			<input
				class="input mono"
				type="text"
				inputmode="numeric"
				placeholder="e.g. 1-10, 15"
				aria-label="Chapter range"
				aria-invalid={rangeError}
				bind:value={range}
				oninput={() => (rangeError = false)}
			/>
			<button class="btn sm" type="submit">Select range</button>
		</form>
		<label class="hide small">
			<input type="checkbox" class="chk" bind:checked={hideDownloaded} />
			Hide downloaded
		</label>
	</div>
	{#if rangeError}
		<p class="error small" role="alert">
			Use chapter numbers from 1 to {chapters.length}, like 1-10, 15.
		</p>
	{/if}
	{#if order.length === 0}
		<p class="empty muted">
			{chapters.length === 0 ? 'The website lists no chapters.' : 'Every chapter is downloaded.'}
		</p>
	{:else}
		<ul
			class="rows"
			role="list"
			aria-label="Chapter list"
			onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
			bind:clientHeight={viewport}
			style:--rows={order.length}
			style:--row="{ROW}px"
		>
			<!-- Gives the list its full scroll height; rows are positioned within it. -->
			<li class="sizer" aria-hidden="true" style:height="{order.length * ROW}px"></li>
			{#each visible as index, i (index)}
				{@const chapter = chapters[index]}
				{#if chapter}
					<li class="row" class:done={chapter.downloaded} style:top="{(first + i) * ROW}px">
						<label>
							<input
								type="checkbox"
								class="chk"
								checked={selected.has(index)}
								aria-label={chapter.name || chapter.link}
								onclick={(e) => pick(e, index)}
							/>
							<span class="mono muted n" aria-hidden="true">{number(index)}</span>
							<span class="name">{chapter.name || chapter.link}</span>
							{#if chapter.downloaded}
								<span class="mark small">✓ downloaded</span>
							{/if}
						</label>
					</li>
				{/if}
			{/each}
		</ul>
	{/if}
</section>

<style>
	.chapters {
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		overflow: hidden;
	}
	header {
		display: flex;
		gap: var(--sp-2);
		align-items: center;
		padding: 10px 14px;
		border-bottom: 1px solid var(--line);
	}
	h2 {
		font-size: var(--fs-lg);
	}
	header .btn {
		margin-left: auto;
	}
	.tools {
		display: flex;
		gap: var(--sp-2) var(--sp-4);
		align-items: center;
		flex-wrap: wrap;
		padding: var(--sp-2) 14px;
		border-bottom: 1px solid var(--line);
	}
	.quick,
	.range,
	.hide {
		display: flex;
		gap: 6px;
		align-items: center;
	}
	.range .input {
		width: 130px;
		padding: 2px 7px;
	}
	.range .input[aria-invalid='true'] {
		border-color: var(--bad);
	}
	.error {
		margin: 0;
		padding: 6px 14px;
		background: var(--bad-soft);
		color: var(--bad);
	}
	.empty {
		margin: 0;
		padding: var(--sp-4) 14px;
	}
	.chk {
		width: 15px;
		height: 15px;
		accent-color: var(--accent);
	}
	/* Fixed-height rows placed absolutely: the list only renders what is in view. */
	.rows {
		position: relative;
		list-style: none;
		margin: 0;
		padding: 0;
		height: min(calc(var(--rows) * var(--row)), 480px);
		overflow-y: auto;
		overscroll-behavior: contain;
	}
	.sizer {
		position: absolute;
		top: 0;
		width: 1px;
		pointer-events: none;
	}
	.row {
		position: absolute;
		left: 0;
		right: 0;
		height: var(--row);
		border-bottom: 1px solid var(--line);
	}
	.row label {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		height: 100%;
		padding: 0 14px;
		font-size: 13px;
		cursor: pointer;
	}
	.row:hover {
		background: var(--surface-2);
	}
	.row.done {
		color: var(--muted);
	}
	.name {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.mark {
		color: var(--ok);
		white-space: nowrap;
	}
	@media (max-width: 860px) {
		.rows {
			height: min(calc(var(--rows) * var(--row)), 60vh);
		}
		.mark {
			font-size: 0;
		}
		.mark::before {
			content: '✓';
			font-size: var(--fs-sm);
		}
	}
</style>
