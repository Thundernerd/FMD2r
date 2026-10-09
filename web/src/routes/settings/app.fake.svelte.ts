import { SvelteURL } from 'svelte/reactivity';

/**
 * Stands in for SvelteKit's `$app/state` and `$app/navigation` in route tests: `goto` moves
 * `page.url`, like the client router does.
 */
export const page = $state({ url: new SvelteURL('http://fmd2r.test/settings') });

export function goto(url: string | URL) {
	page.url = new SvelteURL(url, page.url);
	return Promise.resolve();
}
