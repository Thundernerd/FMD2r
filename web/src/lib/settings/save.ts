import type { FieldProblem } from '#lib/api/types.ts';
import type { Draft } from '#lib/settings/draft.svelte.ts';

/**
 * Shows the fields of a rejected combined save next to their controls: `settings.<path>` on the
 * settings draft, `modules.<id>.<path>` on the open module's draft. Returns the fields neither
 * draft shows, as `field: detail`.
 */
export function showFieldErrors(
	fields: FieldProblem[],
	settings: Draft<object> | null,
	module: { id: string; draft: Draft<object> } | null
): string[] {
	const unplaced: string[] = [];
	const modulePrefix = module ? `modules.${module.id}.` : null;
	for (const { field, detail } of fields) {
		if (settings && field.startsWith('settings.')) {
			settings.errors[field.slice('settings.'.length)] = detail;
		} else if (module && modulePrefix && field.startsWith(modulePrefix)) {
			module.draft.errors[field.slice(modulePrefix.length)] = detail;
		} else {
			unplaced.push(`${field}: ${detail}`);
		}
	}
	return unplaced;
}
