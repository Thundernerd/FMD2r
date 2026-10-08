import type { ModuleOptionSetting } from '#lib/api/types.ts';

/** A choice of a `select` control. */
export interface Choice {
	value: string | number;
	label: string;
}

/** How a setting is edited. `nullable` controls send `null` when left empty. */
export type Control =
	| { kind: 'checkbox' }
	| { kind: 'text'; secret?: boolean; nullable?: boolean; placeholder?: string }
	| { kind: 'number'; min: number; max: number; nullable?: boolean }
	| { kind: 'select'; choices: Choice[] };

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
