import type { Destination, Relocation } from '#lib/api/types.ts';

/** `path` without trailing separators, so `/data/manga/` and `/data/manga` match. */
const bare = (path: string) => path.trim().replace(/[\\/]+$/, '') || path.trim();

/** Whether two folders are the same, ignoring trailing separators. */
export const samePath = (a: string, b: string) => bare(a) === bare(b);

/**
 * The destinations of `saved` whose folder `draft` changes: the same destination, by name
 * (ignoring case), with another folder.
 */
export function changedFolders(saved: Destination[], draft: Destination[]): Relocation[] {
	const key = (d: Destination) => d.name.trim().toLowerCase();
	return saved.flatMap((before) => {
		const after = draft.find((d) => key(d) === key(before));
		if (!after || !after.path.trim() || !before.path.trim() || samePath(before.path, after.path)) {
			return [];
		}
		return [{ from: before.path.trim(), to: after.path.trim() }];
	});
}

/** How the repoint dialog was answered: whether to repoint, and to move the files along. */
export interface RelocateAnswer {
	repoint: boolean;
	moveFiles: boolean;
}

/** The path below `from` when `folder` is inside it, by whole components; else `null`. */
function below(folder: string, from: string): string | null {
	const base = bare(from);
	const path = bare(folder);
	if (path === base) return '';
	const sep = path.charAt(base.length);
	return path.startsWith(base) && (sep === '/' || sep === '\\')
		? path.slice(base.length + 1)
		: null;
}

/**
 * `folder` at the same place under the new folder of the innermost relocation whose old folder
 * holds it, comparing whole path components as fmd-server does (`/data/manga` does not hold
 * `/data/manga2`), with that relocation's index; else `null`.
 */
export function relocatedFolder(
	folder: string,
	relocations: Relocation[]
): { folder: string; index: number } | null {
	let found: { folder: string; index: number } | null = null;
	// Of old folders that both hold `folder`, the longer is inside the other.
	let depth = -1;
	for (const [index, { from, to }] of relocations.entries()) {
		const rest = below(folder, from);
		if (rest === null || bare(from).length <= depth) continue;
		depth = bare(from).length;
		found = { folder: rest ? `${bare(to)}/${rest}` : to, index };
	}
	return found;
}

/** The destination whose folder is `path`, if one is. */
export const destinationAt = (destinations: Destination[], path: string) =>
	destinations.find((d) => samePath(d.path, path));

/** The destination downloads go to when nothing else picks one. */
export const defaultDestination = (destinations: Destination[]) =>
	destinations.find((d) => d.default) ?? destinations[0];

/** How a picker lists a destination. */
export const destinationLabel = (d: Destination) => `${d.name}${d.default ? ' (default)' : ''}`;

/** `base`, or `base 2`, `base 3`, … when another destination has that name (ignoring case). */
export function freeName(destinations: Destination[], base: string): string {
	const taken = (name: string) =>
		destinations.some((d) => d.name.trim().toLowerCase() === name.toLowerCase());
	let name = base;
	for (let n = 2; taken(name); n++) name = `${base} ${n}`;
	return name;
}

/**
 * Why the server would reject `destinations`, keyed by the paths of its field errors
 * (fmd_core's `validate_destinations`): empty or duplicate (ignoring case) names, empty folders,
 * and not exactly one default. Empty when they are valid.
 */
export function destinationErrors(destinations: Destination[]): Record<string, string> {
	const errors: Record<string, string> = {};
	const seen: string[] = [];
	destinations.forEach((d, i) => {
		const name = d.name.trim();
		if (!name) errors[`saveto.destinations.${i}.name`] = 'a destination needs a name';
		else if (seen.includes(name.toLowerCase())) {
			errors[`saveto.destinations.${i}.name`] = `another destination is named ${name}`;
		}
		seen.push(name.toLowerCase());
		if (!d.path.trim()) errors[`saveto.destinations.${i}.path`] = 'a destination needs a folder';
	});
	if (destinations.filter((d) => d.default).length !== 1) {
		errors['saveto.destinations'] = 'exactly one destination must be the default';
	}
	return errors;
}
