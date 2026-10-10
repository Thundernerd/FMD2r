<script lang="ts">
	/** The thumbnail width asked for: a card's 150 px at twice the pixel density. */
	const WIDTH = 300;

	let {
		src,
		title
	}: {
		/** The title's cover (`ListItem.cover_url`); `null` asks for nothing. */
		src: string | null;
		title: string;
	} = $props();

	let visible = $state(false);
	let loaded = $state(false);
	let failed = $state(false);

	const url = $derived(src ? `${src}${src.includes('?') ? '&' : '?'}w=${WIDTH}` : null);
	/** The image is asked for only on screen; one that has not loaded is dropped when the card
	 * scrolls away, which cancels its request. */
	const showImage = $derived(url !== null && !failed && (visible || loaded));

	/** Tracks whether `node` is on screen. Where that cannot be told, it never is, so a long
	 * list does not ask for every cover at once. */
	function onScreen(node: HTMLElement) {
		if (typeof IntersectionObserver === 'undefined') return;
		const observer = new IntersectionObserver((entries) => {
			visible = entries.some((e) => e.isIntersecting);
		});
		observer.observe(node);
		return { destroy: () => observer.disconnect() };
	}

	/** A stable hue per title for its placeholder. */
	function hue(text: string): number {
		let h = 0;
		for (const c of text) h = (h * 31 + c.charCodeAt(0)) % 360;
		return h;
	}
</script>

<span class="frame" use:onScreen>
	{#if !loaded}
		<span class="cover blank" style:--h={hue(title)} aria-hidden="true">{title}</span>
	{/if}
	{#if showImage}
		<img
			class="cover"
			class:loaded
			src={url}
			alt=""
			onload={() => (loaded = true)}
			onerror={() => (failed = true)}
		/>
	{/if}
</span>

<style>
	.frame {
		position: relative;
		display: block;
		width: 100%;
		aspect-ratio: 5 / 7;
		border-radius: 4px;
		overflow: hidden;
	}
	:global(a:hover) > .frame {
		outline: 2px solid var(--accent);
		outline-offset: 2px;
	}
	.cover {
		position: absolute;
		top: 0;
		left: 0;
		width: 100%;
		height: 100%;
	}
	img.cover {
		object-fit: cover;
		opacity: 0;
		transition: opacity 0.2s;
	}
	img.cover.loaded {
		opacity: 1;
	}
	.cover.blank {
		display: flex;
		align-items: flex-end;
		box-sizing: border-box;
		padding: 6px;
		color: var(--on-cover);
		font: 700 var(--fs-ui)/1.1 var(--f-display);
		background: linear-gradient(
			160deg,
			hsl(var(--h) var(--cover-tone-top)),
			hsl(calc(var(--h) + 40) var(--cover-tone-bottom))
		);
		text-shadow: var(--on-cover-shadow);
	}
</style>
