<!--
	PROTOTYPE (#225), branch `prototype/themes` only; never merge into main.

	Picks a theme, accent and mode (built-in ones and the candidates in prototype-themes.css) by
	setting `data-style`, `data-accent` and `data-theme` on `<html>` like `applyAppearance`, keeps the
	pick in the URL (`proto-theme`, `proto-accent`, `proto-mode`), and shows the contrast ratios that
	contrast.test.ts checks for the combination. While it is mounted it wins over the saved settings.
-->
<script lang="ts">
	import { afterNavigate, replaceState } from '$app/navigation';
	import { page } from '$app/state';
	import { onMount } from 'svelte';

	const THEMES = ['default', 'high-contrast', 'warm', 'compact'];
	const CANDIDATE_THEMES = ['oled', 'solarized', 'nord', 'vivid', 'sepia'];
	const ACCENTS = ['teal', 'blue', 'green', 'purple', 'orange', 'red'];
	const CANDIDATE_ACCENTS = ['pink', 'amber', 'indigo', 'cyan', 'lime', 'slate'];
	const ALL_THEMES = [...THEMES, ...CANDIDATE_THEMES];
	type Mode = 'light' | 'dark';

	/** The pairs contrast.test.ts checks, plus the accent rule in tokens.css. */
	const PAIRS: [fg: string, bg: string][] = [
		['--fg', '--bg'],
		['--fg', '--surface'],
		['--muted', '--surface'],
		['--accent-fg', '--accent'],
		['--accent', '--bg'],
		['--accent', '--surface'],
		['--accent', '--accent-soft']
	];

	let style = $state('default');
	let accent = $state('teal');
	let mode = $state<Mode>('light');
	let open = $state(true);
	let ratios = $state<{ pair: string; ratio: number }[]>([]);

	const minimum = $derived(style === 'high-contrast' ? 7 : 4.5);
	const failing = $derived(ratios.filter((r) => r.ratio < minimum).length);

	const PARAMS = { style: 'proto-theme', accent: 'proto-accent', mode: 'proto-mode' } as const;

	let probe: HTMLSpanElement | undefined = $state();

	/** A token's colour as the page draws it now, as linear-light luminance. */
	function luminance(token: string): number {
		if (!probe) return 0;
		probe.style.color = `var(${token})`;
		const [r = 0, g = 0, b = 0] = (getComputedStyle(probe).color.match(/[\d.]+/g) ?? [])
			.slice(0, 3)
			.map((v) => {
				const c = Number(v) / 255;
				return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
			});
		return 0.2126 * r + 0.7152 * g + 0.0722 * b;
	}

	function measure() {
		ratios = PAIRS.map(([fg, bg]) => {
			const [hi = 0, lo = 0] = [luminance(fg), luminance(bg)].sort((x, y) => y - x);
			return { pair: `${fg} on ${bg}`, ratio: Math.round(((hi + 0.05) / (lo + 0.05)) * 100) / 100 };
		});
	}

	/** Sets the attributes, unless they are set already (the observer below calls this too). */
	function apply() {
		const root = document.documentElement;
		if (root.dataset['style'] !== style) root.dataset['style'] = style;
		if (root.dataset['accent'] !== accent) root.dataset['accent'] = accent;
		if (root.dataset['theme'] !== mode) root.dataset['theme'] = mode;
		measure();
	}

	function writeUrl() {
		const url = new URL(page.url.href);
		url.searchParams.set(PARAMS.style, style);
		url.searchParams.set(PARAMS.accent, accent);
		url.searchParams.set(PARAMS.mode, mode);
		if (url.href !== page.url.href) replaceState(url, page.state);
	}

	function pick(next: { style?: string; accent?: string; mode?: Mode }) {
		style = next.style ?? style;
		accent = next.accent ?? accent;
		mode = next.mode ?? mode;
		apply();
		writeUrl();
	}

	function cycleTheme(step: number) {
		const i = ALL_THEMES.indexOf(style);
		pick({ style: ALL_THEMES[(i + step + ALL_THEMES.length) % ALL_THEMES.length] });
	}

	onMount(() => {
		const root = document.documentElement;
		const params = new URL(location.href).searchParams;
		style = params.get(PARAMS.style) ?? root.dataset['style'] ?? 'default';
		accent = params.get(PARAMS.accent) ?? root.dataset['accent'] ?? 'teal';
		const m = params.get(PARAMS.mode) ?? root.dataset['theme'];
		mode =
			m === 'light' || m === 'dark'
				? m
				: matchMedia('(prefers-color-scheme: dark)').matches
					? 'dark'
					: 'light';
		open = matchMedia('(min-width: 861px)').matches;
		apply();
		// The saved settings arrive after the first render (and Settings previews its edits); the
		// switcher's pick wins over both.
		const observer = new MutationObserver(apply);
		observer.observe(root, { attributeFilter: ['data-style', 'data-accent', 'data-theme'] });
		return () => observer.disconnect();
	});

	// Navigating drops the query; put the pick back so the URL can always be shared.
	afterNavigate(() => {
		if (probe) writeUrl();
	});

	function onkeydown(e: KeyboardEvent) {
		const target = e.target as HTMLElement | null;
		if (target?.closest('input, textarea, select, [contenteditable]')) return;
		if (e.key === 'ArrowLeft') cycleTheme(-1);
		else if (e.key === 'ArrowRight') cycleTheme(1);
	}
</script>

<svelte:window {onkeydown} />

{#if import.meta.env.DEV}
	<div class="proto" role="region" aria-label="Theme prototype switcher">
		<span class="probe" bind:this={probe}></span>
		<div class="row">
			<span class="tag">Prototype</span>
			<button class="arrow" type="button" title="Previous theme (←)" onclick={() => cycleTheme(-1)}
				>‹</button
			>
			<select
				aria-label="Theme"
				value={style}
				onchange={(e) => pick({ style: e.currentTarget.value })}
			>
				<optgroup label="Built-in">
					{#each THEMES as t (t)}<option value={t}>{t}</option>{/each}
				</optgroup>
				<optgroup label="Candidates">
					{#each CANDIDATE_THEMES as t (t)}<option value={t}>★ {t}</option>{/each}
				</optgroup>
			</select>
			<button class="arrow" type="button" title="Next theme (→)" onclick={() => cycleTheme(1)}
				>›</button
			>
			<select
				aria-label="Accent"
				value={accent}
				onchange={(e) => pick({ accent: e.currentTarget.value })}
			>
				<optgroup label="Built-in">
					{#each ACCENTS as a (a)}<option value={a}>{a}</option>{/each}
				</optgroup>
				<optgroup label="Candidates">
					{#each CANDIDATE_ACCENTS as a (a)}<option value={a}>★ {a}</option>{/each}
				</optgroup>
			</select>
			<button
				class="mode"
				type="button"
				onclick={() => pick({ mode: mode === 'light' ? 'dark' : 'light' })}
			>
				{mode === 'light' ? '☀ light' : '☾ dark'}
			</button>
			<button
				class="summary"
				class:bad={failing > 0}
				type="button"
				title={open ? 'Hide the contrast readout' : 'Show the contrast readout'}
				onclick={() => (open = !open)}
			>
				{failing > 0 ? `${failing} below ${minimum}:1` : `all ≥ ${minimum}:1`}
				{open ? '▾' : '▸'}
			</button>
		</div>
		{#if open}
			<ul class="ratios">
				{#each ratios as r (r.pair)}
					<li class:bad={r.ratio < minimum}>
						<span>{r.pair.replaceAll('--', '')}</span>
						<b>{r.ratio.toFixed(2)}</b>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
{/if}

<style>
	/* Drawn in the page's own colours swapped round, so it reads as not part of the design. Above the
	 * phone tab bar and the queue dock, so it covers neither. */
	.proto {
		position: fixed;
		left: 8px;
		right: 8px;
		width: fit-content;
		margin: 0 auto;
		bottom: calc(var(--bottom-nav-h) + var(--safe-bottom) + 86px);
		z-index: 60;
		background: var(--fg);
		color: var(--bg);
		border-radius: 14px;
		box-shadow: var(--shadow);
		padding: 6px 10px;
		font: var(--fs-sm) / 1.3 var(--f-mono);
	}
	@media (min-width: 861px) {
		.proto {
			bottom: 112px;
		}
	}
	.probe {
		position: absolute;
		width: 0;
		height: 0;
		overflow: hidden;
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: center;
		gap: 6px;
	}
	.tag {
		font-weight: 500;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		opacity: 0.7;
	}
	select,
	button {
		font: inherit;
		color: inherit;
		background: transparent;
		border: 1px solid currentColor;
		border-radius: 8px;
		padding: 2px 6px;
		cursor: pointer;
	}
	option,
	optgroup {
		color: var(--fg);
		background: var(--surface);
	}
	.arrow {
		border: none;
		font-size: var(--fs-lg);
		padding: 0 4px;
	}
	.bad {
		background: var(--bad);
		color: var(--bg);
		border-color: var(--bad);
		border-radius: 6px;
		padding: 0 4px;
	}
	.ratios {
		list-style: none;
		margin: 6px 0 2px;
		padding: 0;
		display: flex;
		flex-wrap: wrap;
		justify-content: center;
		gap: 4px 12px;
	}
	.ratios li {
		display: flex;
		gap: 4px;
	}
	.ratios li.bad b::after {
		content: ' ✗';
	}
</style>
