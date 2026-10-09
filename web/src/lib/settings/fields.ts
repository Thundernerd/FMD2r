import type { ModuleOptionSetting } from '#lib/api/types.ts';

/** A choice of a `select` control. */
export interface Choice {
	value: string | number;
	label: string;
}

/**
 * How a setting is edited. `nullable` controls send `null` when left empty. A `secret` (a
 * password or token) is write-only: the server only sends whether it is set, as `has_<name>`
 * next to it, and the control sends a value only when one is typed (`''` clears it).
 */
export type Control =
	| { kind: 'checkbox' }
	| { kind: 'text'; nullable?: boolean; placeholder?: string }
	| { kind: 'secret' }
	| { kind: 'number'; min: number; max: number; nullable?: boolean }
	| { kind: 'select'; choices: Choice[] };

/** The path of the `has_<name>` flag that tells whether the secret at `path` is set. */
export function secretFlag(path: string): string {
	const keys = path.split('.');
	const last = keys.pop() ?? '';
	return [...keys, `has_${last}`].join('.');
}

/** One form control, bound to the value at `path` (dot-separated) of a settings object. */
export interface Field {
	path: string;
	label: string;
	help?: string;
	control: Control;
}

/**
 * The controls for a module's `AddOption*` options, in declaration order: a checkbox, an edit
 * field, a spin edit or a drop-down list, as FMD2's website options show them.
 */
export function optionFields(options: ModuleOptionSetting[]): Field[] {
	return options.map((option) => {
		const path = `options.${option.key}`;
		const label = option.caption || option.key;
		switch (option.kind) {
			case 'checkbox':
				return { path, label, control: { kind: 'checkbox' } };
			case 'edit':
				return { path, label, control: { kind: 'text' } };
			case 'spinedit':
				return { path, label, control: { kind: 'number', min: option.min, max: option.max } };
			case 'combobox': {
				const choices: Choice[] = option.items.map((label, value) => ({ value, label }));
				// A stored index past the items (the module dropped some) stays visible and selected.
				if (option.value < 0 || option.value >= option.items.length) {
					choices.push({ value: option.value, label: `(item ${option.value})` });
				}
				return { path, label, control: { kind: 'select', choices } };
			}
		}
	});
}
