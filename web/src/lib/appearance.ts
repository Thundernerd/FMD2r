import type { Accent, AppearanceSettings } from '#lib/api/types.ts';

/** What every `--fs-*` token is multiplied by, per text size (`--text-scale` in tokens.css). */
export const TEXT_SCALE: Record<AppearanceSettings['text_size'], number> = {
	small: 0.9,
	normal: 1,
	large: 1.15,
	larger: 1.3
};

/**
 * Where the applied appearance is cached, for the inline script in `app.html` that applies it
 * before the first paint. It holds the attributes as set on `<html>`, so that script needn't know
 * the settings' values; keep the two in step.
 */
export const APPEARANCE_CACHE_KEY = 'fmd2r.appearance';

/** The attributes an appearance sets on `<html>`; `theme` is `null` to follow the device. */
interface Applied {
	theme: 'light' | 'dark' | null;
	accent: Accent;
	scale: string;
}

function attributes(appearance: AppearanceSettings): Applied {
	return {
		theme: appearance.mode === 'system' ? null : appearance.mode,
		accent: appearance.accent,
		scale: String(TEXT_SCALE[appearance.text_size])
	};
}

/**
 * Shows `appearance`: `data-theme` (none for the device's own light or dark), `data-accent` and
 * `--text-scale` on `<html>`, which tokens.css turns into the colours and font sizes.
 */
export function applyAppearance(appearance: AppearanceSettings) {
	const root = document.documentElement;
	const { theme, accent, scale } = attributes(appearance);
	if (theme) root.dataset['theme'] = theme;
	else delete root.dataset['theme'];
	root.dataset['accent'] = accent;
	root.style.setProperty('--text-scale', scale);
}

/** The saved appearance, and the one being edited in Settings, if any, which shows instead. */
let saved: AppearanceSettings | null = null;
let preview: AppearanceSettings | null = null;

/**
 * Shows `appearance`, an unsaved edit, instead of the saved one; `null` ends the preview and shows
 * the saved one again.
 */
export function previewAppearance(appearance: AppearanceSettings | null) {
	preview = appearance;
	const shown = preview ?? saved;
	if (shown) applyAppearance(shown);
}

/**
 * Adopts `appearance` as the saved one and caches it for the next page load; it shows unless a
 * preview does. Only saved settings are cached: a preview isn't what the next load should show.
 */
export function adoptAppearance(appearance: AppearanceSettings) {
	saved = appearance;
	applyAppearance(preview ?? appearance);
	try {
		localStorage.setItem(APPEARANCE_CACHE_KEY, JSON.stringify(attributes(appearance)));
	} catch {
		// Blocked storage: the next load shows the default look until the settings answer.
	}
}
