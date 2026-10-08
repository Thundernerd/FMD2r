<script lang="ts">
	import { page } from '$app/state';
	import { api, events } from '#lib/app.ts';
	import { ApiError } from '#lib/api/client.ts';
	import type { OutputFormat, SeriesInfo } from '#lib/api/types.ts';
	import ChapterList from '#lib/components/series/ChapterList.svelte';
	import DownloadBox from '#lib/components/series/DownloadBox.svelte';
	import SeriesHeader from '#lib/components/series/SeriesHeader.svelte';

	const module = $derived(page.url.searchParams.get('module') ?? '');
	const link = $derived(page.url.searchParams.get('link') ?? '');

	let series = $state<SeriesInfo | null>(null);
	let error = $state<string | null>(null);
	let website = $state('');
	let selected = $state(new Set<number>());
	let saveTo = $state('');
	let format = $state<OutputFormat | null>(null);

	$effect(() => {
		const [m, l] = [module, link];
		series = null;
		error = null;
		selected = new Set();
		if (!m || !l) {
			error = 'No series given.';
			return;
		}
		let current = true;
		api
			.getSeries(m, l)
			.then((info) => {
				if (current) series = info;
			})
			.catch((e: unknown) => {
				if (current) error = describe(e);
			});
		api
			.listModules()
			.then((modules) => {
				if (current) website = modules.find((x) => x.id === m)?.name ?? m;
			})
			.catch(() => {
				if (current) website = m;
			});
		return () => (current = false);
	});

	// The download box starts from the saved defaults.
	$effect(() => {
		api
			.getSettings()
			.then((settings) => {
				saveTo ||= settings.saveto.default_dir;
				format = settings.output.format;
			})
			.catch(() => {});
	});

	function describe(e: unknown): string {
		if (!(e instanceof ApiError)) return 'Could not reach FMD2r. Try again.';
		if (e.status === 404) return `This series was not found${e.detail ? ` (${e.detail})` : ''}.`;
		if (e.status === 502)
			return `The website could not be reached${e.detail ? `: ${e.detail}` : ''}.`;
		return `Could not load the series (HTTP ${e.status}).`;
	}
</script>

<svelte:head><title>{series?.title ?? 'Series'} · FMD2r</title></svelte:head>

<div class="page">
	<a class="btn ghost back" href="/">← Library</a>
	{#if error}
		<h1>Series</h1>
		<p class="problem" role="alert">{error}</p>
	{:else if !series}
		<p class="muted" aria-live="polite">Loading series…</p>
	{:else}
		<SeriesHeader {series} {website} />
		<div class="body">
			<div class="list">
				<ChapterList chapters={series.chapters} bind:selected />
			</div>
			<aside class="side">
				<DownloadBox
					{api}
					{series}
					{selected}
					bind:saveTo
					{format}
					onqueued={(task) => events.queue.upsert(task)}
				/>
			</aside>
		</div>
	{/if}
</div>

<style>
	.back {
		align-self: flex-start;
	}
	.problem {
		margin: 0;
		padding: var(--sp-3) var(--sp-4);
		border-radius: var(--r-lg);
		background: var(--bad-soft);
		color: var(--bad);
		max-width: 65ch;
	}
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
