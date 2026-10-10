<script lang="ts">
	import type { StepProps } from '#lib/setup/steps.ts';

	let { settings }: StepProps = $props();
</script>

<p>
	FMD2r downloads manga from hundreds of websites, keeps a library of the series you follow and
	downloads their new chapters. It runs the website modules of Free Manga Downloader 2 (FMD2).
</p>
<p>A few questions get it ready:</p>
<ul class="covers">
	<li>where downloads are saved, and in which format;</li>
	<li>which websites to browse in Discover;</li>
	<li>the optional MangaBaka database, for better series details.</li>
</ul>
<p class="small muted">
	{#if settings.general.setup_completed}
		Each step starts from your current settings; change what you like.
	{:else}
		Each choice is saved as you go, and can be changed later in Settings.
	{/if}
</p>

<style>
	p,
	ul {
		margin: 0;
	}
	.covers {
		padding-left: var(--sp-5);
	}
</style>
