import type { Destination } from '#lib/api/types.ts';

/** `path` without trailing separators, so `/data/manga/` and `/data/manga` match. */
const bare = (path: string) => path.trim().replace(/[\\/]+$/, '') || path.trim();

/** Whether two folders are the same, ignoring trailing separators. */
export const samePath = (a: string, b: string) => bare(a) === bare(b);

/** The destination whose folder is `path`, if one is. */
export const destinationAt = (destinations: Destination[], path: string) =>
	destinations.find((d) => samePath(d.path, path));

/** The destination downloads go to when nothing else picks one. */
export const defaultDestination = (destinations: Destination[]) =>
	destinations.find((d) => d.default) ?? destinations[0];

/** The last component of `path`, e.g. the manga folder of a library series. */
export const lastComponent = (path: string) => bare(path).split(/[\\/]/).pop() ?? '';

/** `name` inside folder `dir`. */
export const joinPath = (dir: string, name: string) =>
	name ? `${bare(dir)}${bare(dir).endsWith('/') ? '' : '/'}${name}` : dir;

/** `base`, or `base 2`, `base 3`, … when another destination has that name (ignoring case). */
export function freeName(destinations: Destination[], base: string): string {
	const taken = (name: string) =>
		destinations.some((d) => d.name.trim().toLowerCase() === name.toLowerCase());
	let name = base;
	for (let n = 2; taken(name); n++) name = `${base} ${n}`;
	return name;
}
