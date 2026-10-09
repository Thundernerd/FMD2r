<script lang="ts">
	import type { Snippet } from 'svelte';

	/** The series page's two columns: the chapter list and, beside it, the download box. */
	let { list, side }: { list: Snippet; side: Snippet } = $props();
</script>

<div class="body">
	<div class="list">
		{@render list()}
	</div>
	<aside class="side">
		{@render side()}
	</aside>
</div>

<style>
	.body {
		display: flex;
		gap: 18px;
		align-items: flex-start;
		flex-wrap: wrap;
	}
	.list {
		flex: 1 1 480px;
		min-width: 0;
	}
	.side {
		flex: 0 1 340px;
		min-width: 0;
		position: sticky;
		top: 76px;
	}
	@media (max-width: 860px) {
		.side {
			flex-basis: 100%;
			position: static;
		}
	}
</style>
