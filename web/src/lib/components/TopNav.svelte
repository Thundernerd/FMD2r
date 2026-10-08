<script lang="ts">
	import { page } from '$app/state';
	import type { Snippet } from 'svelte';

	let { children }: { children?: Snippet } = $props();

	// Icons only show in the phone bottom bar.
	const links = [
		{ href: '/', label: 'Library', icon: 'M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z' },
		{
			href: '/discover',
			label: 'Discover',
			icon: 'M11 4a7 7 0 1 0 0 14a7 7 0 1 0 0-14zM20 20l-4-4'
		},
		{ href: '/queue', label: 'Queue', icon: 'M12 4v11M7 10l5 5 5-5M5 20h14' },
		{ href: '/settings', label: 'Settings', icon: 'M4 7h10M18 7h2M4 17h4M12 17h8M16 5v4M10 15v4' },
		{ href: '/system', label: 'System', icon: 'M4 5h16v14H4zM8 10l2 2-2 2M12 15h4' }
	];

	// A series page belongs to the Library section.
	const isCurrent = (href: string): boolean => {
		const path = page.url.pathname;
		if (href === '/') return path === '/' || path === '/series';
		return path === href || path.startsWith(`${href}/`);
	};
</script>

<header class="top">
	<a class="brand" href="/">FMD2r</a>
	<nav class="nav" aria-label="Main">
		{#each links as link (link.href)}
			<a class="nav-link" href={link.href} aria-current={isCurrent(link.href) ? 'page' : undefined}>
				<svg class="nav-icon" viewBox="0 0 24 24" aria-hidden="true"><path d={link.icon} /></svg>
				<span>{link.label}</span>
			</a>
		{/each}
	</nav>
	<div class="actions">
		{@render children?.()}
	</div>
</header>

<style>
	/* Phones first: the nav is a bottom tab bar with icons. */
	.top {
		position: sticky;
		top: var(--safe-top);
		z-index: 20;
		background: var(--surface);
		border-bottom: 1px solid var(--line);
		display: flex;
		align-items: center;
		gap: var(--sp-3);
		padding: 10px var(--sp-4);
	}
	.brand {
		font: 800 18px var(--f-display);
		letter-spacing: -0.02em;
		color: var(--fg);
		text-decoration: none;
	}
	.nav {
		position: fixed;
		left: 0;
		right: 0;
		bottom: 0;
		height: calc(var(--bottom-nav-h) + var(--safe-bottom));
		padding-bottom: var(--safe-bottom);
		background: var(--surface);
		border-top: 1px solid var(--line);
		display: flex;
		justify-content: space-around;
	}
	.nav-link {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 2px;
		padding: 6px 2px;
		font-size: var(--fs-xs);
		font-weight: 500;
		color: var(--muted);
		text-decoration: none;
	}
	.nav-link[aria-current='page'] {
		color: var(--accent);
		font-weight: 600;
	}
	.nav-icon {
		width: 22px;
		height: 22px;
		fill: none;
		stroke: currentColor;
		stroke-width: 1.8;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.actions {
		flex: 1;
		min-width: 0;
		display: flex;
		align-items: center;
		justify-content: flex-end;
		gap: var(--sp-3);
	}

	/* Desktop: the nav sits in the top bar as text pills, like the prototype. */
	@media (min-width: 861px) {
		.top {
			gap: 18px;
			padding: 10px var(--sp-5);
		}
		.brand {
			font-size: 20px;
		}
		.nav {
			position: static;
			height: auto;
			padding: 0;
			background: transparent;
			border: 0;
			gap: 2px;
		}
		.nav-link {
			flex: none;
			display: block;
			padding: 6px 12px;
			border-radius: var(--r-pill);
			font-size: var(--fs-md);
			color: var(--fg);
		}
		.nav-link:hover {
			background: var(--surface-2);
		}
		.nav-link[aria-current='page'] {
			background: var(--fg);
			color: var(--bg);
			font-weight: 500;
		}
		.nav-icon {
			display: none;
		}
	}
</style>
