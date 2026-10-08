/** A JSON value as the settings API sends it. */
export type Json = string | number | boolean | null | Json[] | { [key: string]: Json };
type JsonObject = { [key: string]: Json };

const isObject = (v: unknown): v is JsonObject =>
	typeof v === 'object' && v !== null && !Array.isArray(v);

/** The value at `path` (dot-separated) of `root`, or `undefined` when there is none. */
export function getPath(root: Json, path: string): Json | undefined {
	let node: Json | undefined = root;
	for (const key of path.split('.')) {
		if (!isObject(node)) return undefined;
		node = node[key];
	}
	return node;
}

/** The RFC 7396 merge patch that turns `from` into `to`, or `null` when they are equal. */
function diff(from: Json | undefined, to: Json): Json | undefined {
	if (isObject(from) && isObject(to)) {
		const patch: JsonObject = {};
		for (const [key, value] of Object.entries(to)) {
			const sub = diff(from[key], value);
			if (sub !== undefined) patch[key] = sub;
		}
		return Object.keys(patch).length ? patch : undefined;
	}
	return JSON.stringify(from) === JSON.stringify(to) ? undefined : to;
}

/**
 * An editable copy of a settings object that tracks what changed since it was last saved, and
 * the validation errors the server reported, keyed by the same dotted paths.
 */
export class Draft<T extends object = JsonObject> {
	#saved: JsonObject = $state.raw({});
	#current: JsonObject = $state({});
	/** Validation errors by path; editing a field clears its error. */
	errors: Record<string, string> = $state({});

	constructor(saved: T) {
		this.#adopt(saved);
	}

	#adopt(saved: T) {
		const copy = structuredClone($state.snapshot(saved as unknown)) as JsonObject;
		this.#saved = copy;
		this.#current = structuredClone(copy);
	}

	/** The edited settings. */
	get value(): T {
		return this.#current as T;
	}

	get(path: string): Json | undefined {
		return getPath(this.#current, path);
	}

	set(path: string, value: Json) {
		const keys = path.split('.');
		const last = keys.pop();
		if (last === undefined) return;
		let node: JsonObject = this.#current;
		for (const key of keys) {
			const next = node[key];
			if (!isObject(next)) node[key] = {};
			node = node[key] as JsonObject;
		}
		node[last] = value;
		delete this.errors[path];
	}

	/** Whether the value at `path` differs from the saved one. */
	isDirty(path: string): boolean {
		return diff(getPath(this.#saved, path), this.get(path) ?? null) !== undefined;
	}

	get dirty(): boolean {
		return this.changes() !== null;
	}

	/** The edits as a merge patch over the saved settings, or `null` when there are none. */
	changes(): JsonObject | null {
		const patch = diff(this.#saved, $state.snapshot(this.#current as unknown) as JsonObject);
		return isObject(patch) ? patch : null;
	}

	/** Drops the edits and errors. */
	reset() {
		this.#current = structuredClone(this.#saved);
		this.errors = {};
	}

	/** Adopts `saved`, what the server holds after a save, as the new baseline. */
	commit(saved: T) {
		this.#adopt(saved);
		this.errors = {};
	}
}
