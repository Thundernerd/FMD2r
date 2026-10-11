<!-- PROTOTYPE (#224): C's omnibox: paste a URL, jump to a page or favorite, search Discover. -->
<script lang="ts">
	import { goto } from '$app/navigation';
	import type { Api } from '#lib/api/client.ts';
	import type { FavoriteView } from '#lib/api/types.ts';
	import { seriesHref } from '#lib/series/href.ts';

	let { api }: { api: Api } = $props();

	type Option = { label: string; hint: string; run: () => Promise<unknown> | void };

	const PAGES = [
		{ label: 'Home', href: '/' },
		{ label: 'Library', href: '/?library' },
		{ label: 'Discover', href: '/discover' },
		{ label: 'Queue', href: '/queue' },
		{ label: 'Settings', href: '/settings' },
		{ label: 'System', href: '/system' }
	];

	let text = $state('');
	let open = $state(false);
	let active = $state(0);
	let error = $state<string | null>(null);
	let input: HTMLInputElement | undefined = $state();
	let favorites = $state<FavoriteView[]>([]);

	$effect(() => {
		api
			.listFavorites()
			.then((list) => (favorites = list))
			.catch(() => {});
	});

	const looksLikeUrl = (s: string): boolean =>
		/^[a-z]+:\/\//i.test(s) || /^[\w-]+\.[\w.-]+\//.test(s);

	async function add(typed: string) {
		const value = /^[a-z][a-z\d+.-]*:\/\//i.test(typed) ? typed : `https://${typed}`;
		try {
			const series = await api.resolveUrl(value);
			if (!series) {
				error = 'No module handles this URL.';
				return;
			}
			await goto(seriesHref(series));
		} catch {
			error = 'Could not reach FMD2r. Try again.';
		}
	}

	const options = $derived.by((): Option[] => {
		const q = text.trim();
		const lower = q.toLowerCase();
		const list: Option[] = [];
		if (q && looksLikeUrl(q)) list.push({ label: `Add ${q}`, hint: 'URL', run: () => add(q) });
		for (const p of PAGES) {
			if (!q || p.label.toLowerCase().includes(lower)) {
				list.push({ label: p.label, hint: 'Go to', run: () => goto(p.href) });
			}
		}
		if (q) {
			for (const f of favorites.filter((f) => f.title.toLowerCase().includes(lower)).slice(0, 6)) {
				list.push({ label: f.title, hint: f.website, run: () => goto(seriesHref(f)) });
			}
			if (!looksLikeUrl(q)) {
				list.push({
					label: `Search all websites for “${q}”`,
					hint: 'Discover',
					run: () => goto(`/discover?${new URLSearchParams({ q })}`)
				});
			}
		}
		return list;
	});

	function pick(option: Option | undefined) {
		if (!option) return;
		open = false;
		text = '';
		input?.blur();
		option.run();
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'ArrowDown') active = Math.min(active + 1, options.length - 1);
		else if (event.key === 'ArrowUp') active = Math.max(active - 1, 0);
		else if (event.key === 'Enter') pick(options[active]);
		else if (event.key === 'Escape') {
			open = false;
			input?.blur();
			return;
		} else return;
		event.preventDefault();
	}

	function onWindowKeydown(event: KeyboardEvent) {
		const typing =
			event.target instanceof HTMLInputElement ||
			event.target instanceof HTMLTextAreaElement ||
			event.target instanceof HTMLSelectElement;
		if ((event.key === '/' && !typing) || (event.key === 'k' && (event.ctrlKey || event.metaKey))) {
			event.preventDefault();
			input?.focus();
		}
	}
</script>

<svelte:window onkeydown={onWindowKeydown} />

<div class="omni">
	<input
		class="input"
		type="text"
		placeholder="Paste a URL, go to a page or series, or search all websites"
		aria-label="Command"
		bind:this={input}
		bind:value={text}
		onfocus={() => (open = true)}
		onblur={() => setTimeout(() => (open = false), 150)}
		oninput={() => {
			active = 0;
			error = null;
			open = true;
		}}
		onkeydown={onKeydown}
	/>
	<kbd class="small muted">/</kbd>
	{#if error}
		<p class="error small" role="alert">{error}</p>
	{/if}
	{#if open && options.length > 0}
		<ul class="results" role="listbox">
			{#each options as option, i (option.hint + option.label)}
				<li role="option" aria-selected={i === active}>
					<button
						type="button"
						class:active={i === active}
						onmousedown={(e) => e.preventDefault()}
						onclick={() => pick(option)}
					>
						<span>{option.label}</span><span class="small muted">{option.hint}</span>
					</button>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	.omni {
		position: relative;
		flex: 1;
		min-width: 0;
		max-width: 640px;
	}
	.input {
		width: 100%;
		padding: 9px 36px 9px 14px;
		border-radius: var(--r-pill);
		font-size: var(--fs-search);
	}
	kbd {
		position: absolute;
		right: 14px;
		top: 50%;
		transform: translateY(-50%);
		font-family: var(--f-mono);
	}
	.results {
		position: absolute;
		left: 0;
		right: 0;
		top: calc(100% + 6px);
		z-index: 50;
		list-style: none;
		margin: 0;
		padding: var(--sp-1);
		max-height: 60vh;
		overflow: auto;
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		box-shadow: var(--shadow);
	}
	.results button {
		width: 100%;
		display: flex;
		justify-content: space-between;
		gap: var(--sp-3);
		padding: 7px 10px;
		border: 0;
		border-radius: var(--r);
		background: transparent;
		text-align: left;
	}
	.results button.active {
		background: var(--accent-soft);
	}
	.error {
		position: absolute;
		top: calc(100% + 6px);
		left: 0;
		margin: 0;
		padding: 6px 10px;
		border-radius: var(--r);
		background: var(--bad-soft);
		color: var(--bad);
		z-index: 51;
	}
	/* On phones the omnibox sits at the bottom, so results open upwards. */
	@media (max-width: 860px) {
		.results,
		.error {
			top: auto;
			bottom: calc(100% + 6px);
		}
	}
</style>
