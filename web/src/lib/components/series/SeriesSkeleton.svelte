<script lang="ts">
	import SeriesLayout from './SeriesLayout.svelte';

	/** Placeholder chapter rows; enough to fill the list's first screen. */
	const ROWS = 8;
</script>

<!-- Laid out like SeriesHeader, ChapterList and DownloadBox, so the page doesn't jump when they arrive. -->
<div aria-busy="true">
	<div class="shapes" aria-hidden="true">
		<div class="hero">
			<div class="shape cover"></div>
			<div class="meta">
				<div class="kicker">
					<span class="shape line w-80"></span>
					<span class="shape pill"></span>
				</div>
				<span class="shape title"></span>
				<span class="shape line w-280"></span>
				<div class="facts">
					<span class="shape fact"></span>
					<span class="shape fact"></span>
					<span class="shape fact"></span>
				</div>
				<div class="genres">
					<span class="shape pill wide"></span>
					<span class="shape pill wide"></span>
					<span class="shape pill wide"></span>
				</div>
				<div class="summary">
					<span class="shape line w-full"></span>
					<span class="shape line w-full"></span>
					<span class="shape line w-full"></span>
					<span class="shape line w-60pc"></span>
				</div>
				<div class="actions">
					<span class="shape button"></span>
					<span class="shape button"></span>
				</div>
			</div>
		</div>
		<SeriesLayout>
			{#snippet list()}
				<div class="card">
					<div class="strip">
						<span class="shape line w-80"></span>
						<span class="shape button sm end"></span>
					</div>
					<div class="strip">
						<span class="shape button sm"></span>
						<span class="shape button sm"></span>
						<span class="shape button sm"></span>
					</div>
					{#each { length: ROWS }, i (i)}
						<div class="row">
							<span class="shape check"></span>
							<span class="shape line w-120"></span>
						</div>
					{/each}
				</div>
			{/snippet}
			{#snippet side()}
				<div class="card box">
					<span class="shape line w-140"></span>
					<span class="shape line w-60"></span>
					<span class="shape field"></span>
					<span class="shape line w-60"></span>
					<span class="shape line w-120"></span>
					<span class="shape button"></span>
				</div>
			{/snippet}
		</SeriesLayout>
	</div>
</div>

<style>
	.shapes {
		display: flex;
		flex-direction: column;
		gap: 18px;
	}
	.shape {
		display: block;
		flex: none;
		border-radius: var(--r);
		background: linear-gradient(90deg, var(--surface-2) 0%, var(--line) 50%, var(--surface-2) 100%)
			0 0 / 200% 100%;
		animation: shimmer 1.6s ease-in-out infinite;
	}
	@keyframes shimmer {
		from {
			background-position: 100% 0;
		}
		to {
			background-position: -100% 0;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.shape {
			animation: none;
			background: var(--surface-2);
		}
	}

	/* SeriesHeader */
	.hero {
		display: flex;
		gap: 28px;
		flex-wrap: wrap;
	}
	.cover {
		width: 180px;
		height: 255px;
	}
	.meta {
		flex: 1 1 340px;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.kicker,
	.facts,
	.strip,
	.row {
		display: flex;
		gap: var(--sp-2);
		align-items: center;
	}
	.facts {
		gap: 22px;
		flex-wrap: wrap;
	}
	.genres {
		display: flex;
		gap: 6px;
		flex-wrap: wrap;
	}
	.actions {
		display: flex;
		gap: var(--sp-2);
	}
	.summary {
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 4px 0;
		max-width: 65ch;
	}
	.line {
		height: 12px;
	}
	.w-60 {
		width: 60px;
	}
	.w-80 {
		width: 80px;
	}
	.w-120 {
		width: 120px;
	}
	.w-140 {
		width: 140px;
	}
	.w-280 {
		width: 280px;
		max-width: 100%;
	}
	.w-full {
		width: 100%;
	}
	.w-60pc {
		width: 60%;
	}
	.pill {
		width: 64px;
		height: 18px;
		border-radius: var(--r-pill);
	}
	.pill.wide {
		width: 76px;
		height: 22px;
	}
	.title {
		width: 60%;
		max-width: 420px;
		height: 38px;
	}
	.fact {
		width: 90px;
		height: 34px;
	}
	.button {
		width: 140px;
		height: 30px;
	}
	.button.sm {
		width: 56px;
		height: 24px;
	}
	.end {
		margin-left: auto;
	}

	/* ChapterList and DownloadBox */
	.card {
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		overflow: hidden;
	}
	.strip {
		padding: 10px 14px;
		border-bottom: 1px solid var(--line);
	}
	.row {
		height: 36px;
		padding: 0 14px;
		gap: 10px;
	}
	.check {
		width: 15px;
		height: 15px;
		border-radius: 3px;
	}
	.box {
		display: flex;
		flex-direction: column;
		gap: 10px;
		padding: 12px 14px;
	}
	.field {
		height: 30px;
	}

	@media (max-width: 860px) {
		.hero {
			gap: var(--sp-4);
		}
		.cover {
			width: 120px;
			height: 170px;
		}
	}
</style>
