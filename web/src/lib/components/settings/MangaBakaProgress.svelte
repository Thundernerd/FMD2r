<script lang="ts">
	import type { MangaBakaDownload } from '#lib/mangabaka.svelte.ts';

	/** A running MangaBaka download's progress bar and step. */
	let { download }: { download: MangaBakaDownload } = $props();
</script>

<div class="progress">
	<div
		class="bar"
		role="progressbar"
		aria-label="Download progress"
		aria-valuemin="0"
		aria-valuemax="100"
		aria-valuenow={download.percent ?? undefined}
	>
		<i style:width="{download.percent ?? 0}%"></i>
	</div>
	<span class="small muted">
		{download.event?.phase === 'matching'
			? 'Matching the lists…'
			: download.event?.status_text || 'Starting…'}
	</span>
</div>

<style>
	.progress {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
	}
</style>
