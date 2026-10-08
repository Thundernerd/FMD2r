<script lang="ts">
	import { page } from '$app/state';

	const module = $derived(page.params.module ?? '');
	const link = $derived(page.params.link ?? '');
</script>

<svelte:head><title>{link} · FMD2r</title></svelte:head>

<div class="page">
	<a class="btn ghost back" href="/">← Library</a>
	<div class="label">{module}</div>
	<h1>{link}</h1>
	<p class="placeholder">Series info, chapter list and the download box. Arrives with T24.</p>
</div>

<style>
	.back {
		align-self: flex-start;
	}
</style>
