<script lang="ts">
	import type { Api } from '#lib/api/client.ts';
	import { ApiError } from '#lib/api/client.ts';
	import type { SeriesInfo } from '#lib/api/types.ts';

	let {
		api,
		series,
		selected,
		onmarked
	}: {
		api: Api;
		series: SeriesInfo;
		selected: Set<number>;
		/** The chapters at `indices` of the series at `link` are now `seen`, as the server stored them. */
		onmarked: (
			series: { module_id: string; link: string },
			indices: number[],
			seen: boolean
		) => void;
	} = $props();

	let busy = $state(false);
	let error = $state<string | null>(null);

	const count = $derived(selected.size);

	/** Marks the selected chapters seen, or clears their marks; no file is touched. */
	async function mark(seen: boolean) {
		if (count === 0 || busy) return;
		const picked = [...selected].sort((a, b) => a - b);
		const { module_id, link } = series;
		busy = true;
		error = null;
		try {
			await api.markChaptersSeen({
				module_id,
				link,
				chapters: picked.flatMap((i) => series.chapters[i]?.link ?? []),
				seen
			});
			onmarked({ module_id, link }, picked, seen);
		} catch (e) {
			error =
				e instanceof ApiError
					? `The chapters could not be marked (HTTP ${e.status}).`
					: 'Could not reach FMD2r. Try again.';
		} finally {
			busy = false;
		}
	}
</script>

<section class="box" aria-label="Seen">
	<div class="actions">
		<button class="btn" type="button" disabled={count === 0 || busy} onclick={() => mark(true)}>
			Mark seen
		</button>
		<button class="btn" type="button" disabled={count === 0 || busy} onclick={() => mark(false)}>
			Mark unseen
		</button>
	</div>
	{#if error}
		<p class="error small" role="alert">{error}</p>
	{/if}
</section>

<style>
	.box {
		display: flex;
		flex-direction: column;
		gap: 10px;
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		padding: 12px 14px;
	}
	.actions {
		display: flex;
		gap: var(--sp-2);
		flex-wrap: wrap;
	}
	.error {
		margin: 0;
		padding: 6px 10px;
		border-radius: var(--r);
		background: var(--bad-soft);
		color: var(--bad);
	}
</style>
