// PROTOTYPE (#224), throwaway: see PrototypeSwitcher.svelte for the question and the variants.
import { page } from '$app/state';

export const VARIANTS = [
	{ key: 'B', name: 'Library (today)' },
	{ key: 'S', name: 'Sidebar' },
	{ key: 'W', name: 'Workbench' },
	{ key: 'C', name: 'Command' },
	{ key: 'R', name: 'Queue rail' }
] as const;

export type VariantKey = (typeof VARIANTS)[number]['key'];

const isKey = (v: string | null): v is VariantKey => VARIANTS.some((x) => x.key === v);

/** The last variant picked, so links that drop `?variant=` keep it. */
export const picked = $state<{ key: VariantKey }>({ key: 'B' });

/** The variant in the URL, else the last one picked. */
export function currentVariant(): VariantKey {
	const v = page.url.searchParams.get('variant');
	return isKey(v) ? v : picked.key;
}

/** Whether the URL carries a valid `?variant=`. */
export function urlHasVariant(url: {
	searchParams: { get(name: string): string | null };
}): boolean {
	return isKey(url.searchParams.get('variant'));
}

/** Only in dev or mock builds, so a stray merge can't ship the switcher. */
export const prototypesOn = import.meta.env.DEV || import.meta.env.VITE_API_MOCK === 'true';

/** The main nav, shared by the variants that draw their own (TopNav only draws the B/R bar). */
export const NAV = [
	{ href: '/', label: 'Library', icon: 'M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z' },
	{ href: '/discover', label: 'Discover', icon: 'M11 4a7 7 0 1 0 0 14a7 7 0 1 0 0-14zM20 20l-4-4' },
	{ href: '/queue', label: 'Queue', icon: 'M12 4v11M7 10l5 5 5-5M5 20h14' },
	{ href: '/settings', label: 'Settings', icon: 'M4 7h10M18 7h2M4 17h4M12 17h8M16 5v4M10 15v4' },
	{ href: '/system', label: 'System', icon: 'M4 5h16v14H4zM8 10l2 2-2 2M12 15h4' }
];

export function isCurrent(href: string): boolean {
	const path = page.url.pathname;
	if (href === '/') return path === '/' || path === '/series';
	return path === href || path.startsWith(`${href}/`);
}
