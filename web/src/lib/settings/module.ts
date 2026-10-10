import type { ModuleSettingsView } from '#lib/api/types.ts';

/** The editable part of a module's settings, shaped like its PATCH body. */
export const editable = (view: ModuleSettingsView) => ({
	enabled: view.enabled,
	limits: view.limits,
	http: view.http,
	save_to: view.save_to,
	options: Object.fromEntries(view.options.map((o) => [o.key, o.value]))
});
